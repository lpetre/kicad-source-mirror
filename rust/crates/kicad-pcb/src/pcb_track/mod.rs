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
//!   C++ class overrides (for `PcbArc` / `PcbVia` also the `PCB_TRACK` overrides they inherit,
//!   forwarded to their own inherent method because the `PCB_TRACK` bodies test `Type()`);
//! * the inherent methods in the module of the owning package: `PcbTrack` here (M16), `PcbArc`
//!   in `arc.rs` (M16), `PcbVia` in `via.rs` and its shapes in `shapes.rs` (M17); bodies are
//!   `port_pending!` stubs unless trivial.

pub mod arc;
pub mod shapes;
pub mod via;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{
    KicadT, Kiid, KiidGenerator, Lset, MAX_CU_LAYERS, PCB_IU_SCALE, PcbLayerId, StrokeParams,
};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapePolySet, ShapeSegment, Vector2I};
use std::sync::Arc;

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::ids::NetCode;
use crate::item::{BoardItemBase, ConnectedItemBase, RecurseMode, SharedShape};
use crate::item_ref::ItemRef;
use crate::netclass::Netclass;
use crate::padstack::Padstack;
use crate::traits::{BoardConnectedItem, BoardItem, BoardItemMut, Flashing, Invalidate};

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

    /// `PCB_TRACK::IsOnLayer()`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PCB_TRACK::IsOnLayer", ctx, layer)
    }

    /// `PCB_TRACK::GetLayerSet()`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        crate::port_pending!("PCB_TRACK::GetLayerSet", ctx)
    }

    /// `PCB_TRACK::SetLayerSet()`.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        crate::port_pending!("PCB_TRACK::SetLayerSet", env, layers)
    }

    /// `PCB_TRACK::GetBoundingBox()`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TRACK::GetBoundingBox", ctx)
    }

    /// `PCB_TRACK::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_TRACK::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_TRACK::TransformShapeToPolygon()`.
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
        crate::port_pending!(
            "PCB_TRACK::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_TRACK::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TRACK::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_TRACK::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_TRACK::HitTest (rect)", ctx, rect, contained, accuracy)
    }

    /// `PCB_TRACK::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_TRACK::Rotate", env, centre, angle)
    }

    /// `PCB_TRACK::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TRACK::Flip", env, centre, dir)
    }

    /// `PCB_TRACK::Mirror( aCentre, aFlipDirection )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TRACK::Mirror", env, centre, dir)
    }

    /// `PCB_TRACK::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_TRACK::GetItemDescription", ctx, units, full)
    }

    /// `PCB_TRACK::GetFriendlyName()`.
    pub fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_TRACK::GetFriendlyName", ctx)
    }
}
