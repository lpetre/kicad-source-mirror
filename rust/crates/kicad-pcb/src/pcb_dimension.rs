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

//! Port of `pcbnew/pcb_dimension.h` and `pcbnew/pcb_dimension.cpp` (`PCB_DIMENSION_BASE` and
//! the five `PCB_DIM_*` classes, one struct with the concrete class as `kind`): data,
//! constructors and delegation shims (package M0c), the overrides (stubs for package M12).
//!
//! `PcbDimension` is its own `EDA_TEXT` host (`PCB_DIMENSION_BASE : PCB_TEXT`,
//! `board-model.md` 6.2a); the `EDA_TEXT` virtuals that `PCB_TEXT` overrides and the dimension
//! inherits call the embedded `PcbText` (no dimension class overrides a virtual they reach).
//! Dropped: `m_inClearRenderCache`, `m_busy` (re-entrancy guards of the C++ virtual-call
//! chain).

use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{EdaUnits, KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapePolySet, Vector2I};

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_text::EdaText;
use crate::item::{BoardItemBase, SharedShape};
use crate::pcb_text::PcbText;
use crate::traits::{BoardItem, BoardItemMut, EdaTextItem, Flashing, Invalidate};

/// `DIM_UNITS_FORMAT`: how to display the units in a dimension's text; the values are written
/// to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimUnitsFormat {
    /// `1234.0`
    NoSuffix = 0,
    /// `1234.0 mm`
    BareSuffix,
    /// `1234.0 (mm)`
    ParenSuffix,
}

/// `DIM_PRECISION`; the values are written to files. The names are the digit patterns of the
/// C++ enumerators (`V_*`: 0.00 / 0 / 0.0 depending on the units).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimPrecision {
    X = 0,
    X_X,
    X_XX,
    X_XXX,
    X_XXXX,
    X_XXXXX,
    V_VV,
    V_VVV,
    V_VVVV,
    V_VVVVV,
}

/// `DIM_TEXT_POSITION`: where to place the text on a dimension; the values are written to
/// files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimTextPosition {
    /// Text appears outside the dimension line (default).
    Outside = 0,
    /// Text appears in line with the dimension line.
    Inline,
    /// Text placement is manually set by the user.
    Manual,
}

/// `DIM_UNITS_MODE`: the units selection as stored in the file (`EDA_UNITS` alone does not
/// cut it).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimUnitsMode {
    Inch = 0,
    Mils,
    Mm,
    Automatic,
}

/// `DIM_ARROW_DIRECTION`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimArrowDirection {
    /// `>-----<`
    Inward = 0,
    /// `<----->`
    Outward,
}

/// `DIM_TEXT_BORDER`: frame to show around dimension text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimTextBorder {
    None = 0,
    Rectangle,
    Circle,
    Roundrect,
}

/// `PCB_DIM_ORTHOGONAL::DIR`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimOrthogonalDir {
    Horizontal = 0,
    Vertical,
}

/// `PCB_DIMENSION_BASE : PCB_TEXT`, with the five concrete classes as `kind`.
#[derive(Clone, Debug)]
pub struct PcbDimension {
    /// The `PCB_TEXT` part.
    pub text: PcbText,
    /// m_overrideTextEnabled: manually specify the displayed measurement value.
    pub override_text_enabled: bool,
    /// m_valueString: displayed value when the override is on.
    pub value_string: String,
    pub prefix: String,  // m_prefix
    pub suffix: String,  // m_suffix
    pub units: EdaUnits, // m_units
    /// m_autoUnits: follow the currently selected UI units.
    pub auto_units: bool,
    pub units_format: DimUnitsFormat,       // m_unitsFormat
    pub arrow_direction: DimArrowDirection, // m_arrowDirection
    pub precision: DimPrecision,            // m_precision
    pub suppress_zeroes: bool,              // m_suppressZeroes
    pub line_thickness: i32,                // m_lineThickness
    pub arrow_length: i32,                  // m_arrowLength
    pub extension_offset: i32,              // m_extensionOffset
    pub text_position: DimTextPosition,     // m_textPosition
    pub keep_text_aligned: bool,            // m_keepTextAligned
    pub measured_value: i32,                // m_measuredValue
    /// m_start: FP-relative inside a footprint, board absolute otherwise.
    pub(crate) lib_start: Vector2I,
    /// m_end: FP-relative inside a footprint, board absolute otherwise.
    pub(crate) lib_end: Vector2I,
    /// m_shapes: derived by `update()`, board frame.
    pub shapes: Vec<SharedShape>,
    pub kind: DimensionKind,
}

/// The concrete `PCB_DIM_*` class and its own members.
#[derive(Clone, Debug)]
pub enum DimensionKind {
    /// `PCB_DIM_ALIGNED_T`.
    Aligned(DimAligned),
    /// `PCB_DIM_ORTHOGONAL_T` (`: PCB_DIM_ALIGNED`); `orientation` = m_orientation.
    Orthogonal {
        aligned: DimAligned,
        orientation: DimOrthogonalDir,
    },
    /// `PCB_DIM_RADIAL_T`; m_leaderLength.
    Radial { leader_length: i32 },
    /// `PCB_DIM_LEADER_T`; m_textBorder.
    Leader { text_border: DimTextBorder },
    /// `PCB_DIM_CENTER_T`.
    Center,
}

/// The `PCB_DIM_ALIGNED` members.
#[derive(Clone, Debug, PartialEq)]
pub struct DimAligned {
    /// m_height: perpendicular distance from features to crossbar.
    pub height: i32,
    /// m_extensionHeight: length of extension lines past the crossbar.
    pub extension_height: i32,
    pub cross_bar_start: Vector2I, // m_crossBarStart
    pub cross_bar_end: Vector2I,   // m_crossBarEnd
}

/// `s_arrowAngle` (pcb_dimension.cpp).
const ARROW_ANGLE_DEGREES: f64 = 27.5;

impl DimAligned {
    /// `PCB_DIM_ALIGNED( aParent, aType )`: height 0, extension height from the arrow length
    /// "to preserve look of old dimensions".
    fn for_arrow_length(arrow_length: i32) -> Self {
        DimAligned {
            height: 0,
            extension_height: (arrow_length as f64
                * EdaAngle::from_degrees(ARROW_ANGLE_DEGREES).sin())
                as i32,
            cross_bar_start: Vector2I::default(),
            cross_bar_end: Vector2I::default(),
        }
    }
}

impl Default for PcbDimension {
    /// `PCB_DIM_ALIGNED( nullptr, PCB_DIM_ALIGNED_T )` with a nil UUID.
    fn default() -> Self {
        PcbDimension::of_type(KicadT::PcbDimAlignedT).expect("aligned dimension")
    }
}

impl PcbDimension {
    /// `PCB_DIMENSION_BASE( aParent, aType )`: the base values, `kind` still to be set.
    fn base_values(kind: DimensionKind) -> Self {
        let mut text = PcbText::default();
        text.base.layer = PcbLayerId::DwgsUser; // m_layer = Dwgs_User
        PcbDimension {
            text,
            override_text_enabled: false,
            value_string: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            units: EdaUnits::Inch,
            auto_units: false,
            units_format: DimUnitsFormat::BareSuffix,
            arrow_direction: DimArrowDirection::Outward,
            precision: DimPrecision::X_XXXX,
            suppress_zeroes: false,
            line_thickness: PCB_IU_SCALE.mm_to_iu(0.2),
            arrow_length: PCB_IU_SCALE.mils_to_iu(50),
            extension_offset: 0,
            text_position: DimTextPosition::Outside,
            keep_text_aligned: true,
            measured_value: 0,
            lib_start: Vector2I::default(),
            lib_end: Vector2I::default(),
            shapes: Vec::new(),
            kind,
        }
    }

    /// The constructor of the `PCB_DIM_*` class `type_` (`PCB_DIM_ALIGNED_T`,
    /// `PCB_DIM_ORTHOGONAL_T`, `PCB_DIM_RADIAL_T`, `PCB_DIM_LEADER_T`, `PCB_DIM_CENTER_T`) with a
    /// nil UUID; `None` for any other type.
    pub fn of_type(type_: KicadT) -> Option<Self> {
        let mut d = PcbDimension::base_values(DimensionKind::Center);
        let arrow_length = d.arrow_length;
        match type_ {
            KicadT::PcbDimAlignedT => {
                d.kind = DimensionKind::Aligned(DimAligned::for_arrow_length(arrow_length));
            }
            KicadT::PcbDimOrthogonalT => {
                d.kind = DimensionKind::Orthogonal {
                    aligned: DimAligned::for_arrow_length(arrow_length),
                    orientation: DimOrthogonalDir::Horizontal,
                };
            }
            KicadT::PcbDimRadialT => {
                d.units_format = DimUnitsFormat::NoSuffix;
                d.override_text_enabled = false;
                d.keep_text_aligned = true;
                d.prefix = "R ".to_string();
                d.kind = DimensionKind::Radial {
                    leader_length: arrow_length * 3,
                };
            }
            KicadT::PcbDimLeaderT => {
                d.units_format = DimUnitsFormat::NoSuffix;
                d.override_text_enabled = true;
                d.keep_text_aligned = false;
                d.value_string = "Leader".to_string(); // SetOverrideText( _( "Leader" ) )
                d.kind = DimensionKind::Leader {
                    text_border: DimTextBorder::None,
                };
            }
            KicadT::PcbDimCenterT => {
                d.units_format = DimUnitsFormat::NoSuffix;
                d.override_text_enabled = true;
            }
            _ => return None,
        }
        Some(d)
    }

    /// `new PCB_DIM_*( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator, type_: KicadT) -> Option<Self> {
        let mut d = PcbDimension::of_type(type_)?;
        d.text.base.uuid = Kiid::new(kiid_generator);
        Some(d)
    }

    /// `Type()` of the concrete class.
    pub fn dimension_type(&self) -> KicadT {
        match self.kind {
            DimensionKind::Aligned(_) => KicadT::PcbDimAlignedT,
            DimensionKind::Orthogonal { .. } => KicadT::PcbDimOrthogonalT,
            DimensionKind::Radial { .. } => KicadT::PcbDimRadialT,
            DimensionKind::Leader { .. } => KicadT::PcbDimLeaderT,
            DimensionKind::Center => KicadT::PcbDimCenterT,
        }
    }

    /// `GetClass()` of the concrete class.
    pub fn dimension_class(&self) -> &'static str {
        match self.kind {
            DimensionKind::Aligned(_) => "PCB_DIM_ALIGNED",
            DimensionKind::Orthogonal { .. } => "PCB_DIM_ORTHOGONAL",
            DimensionKind::Radial { .. } => "PCB_DIM_RADIAL",
            DimensionKind::Leader { .. } => "PCB_DIM_LEADER",
            DimensionKind::Center => "PCB_DIM_CENTER",
        }
    }

    /// The stored `m_start` (lib frame inside a footprint); the writer reads it.
    pub fn lib_start(&self) -> Vector2I {
        self.lib_start
    }

    /// The stored `m_end` (lib frame inside a footprint).
    pub fn lib_end(&self) -> Vector2I {
        self.lib_end
    }

    /// Parser: store `m_start` / `m_end` directly.
    pub fn set_lib_points(&mut self, start: Vector2I, end: Vector2I) {
        self.lib_start = start;
        self.lib_end = end;
    }

    /// `GetStart()` (virtual, introduced by `PCB_DIMENSION_BASE`): board frame.
    pub fn start(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_DIMENSION_BASE::GetStart", ctx)
    }

    /// `SetStart()`.
    pub fn set_start(&mut self, env: &MutEnv, point: Vector2I) {
        crate::port_pending!("PCB_DIMENSION_BASE::SetStart", env, point)
    }

    /// `GetEnd()`: board frame.
    pub fn end(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_DIMENSION_BASE::GetEnd", ctx)
    }

    /// `SetEnd()`.
    pub fn set_end(&mut self, env: &MutEnv, point: Vector2I) {
        crate::port_pending!("PCB_DIMENSION_BASE::SetEnd", env, point)
    }

    /// `Update()`: `updateGeometry()` + `updateText()` of the concrete class (reads the board
    /// for text variables and units: clone-edit-store, `board-model.md` 6.4).
    pub fn update(&mut self, ctx: &BoardCtx<'_>) {
        crate::port_pending!("PCB_DIMENSION_BASE::Update", ctx)
    }

    /// `updateGeometry()` of the concrete class (pure virtual in `PCB_DIMENSION_BASE`).
    pub fn update_geometry(&mut self, ctx: &BoardCtx<'_>) {
        crate::port_pending!("PCB_DIM_*::updateGeometry", ctx)
    }

    /// `updateText()` of the concrete class.
    pub fn update_text(&mut self, ctx: &BoardCtx<'_>) {
        crate::port_pending!("PCB_DIMENSION_BASE::updateText", ctx)
    }

    /// `GetValueText()`.
    pub fn value_text(&self) -> String {
        crate::port_pending!("PCB_DIMENSION_BASE::GetValueText")
    }

    /// `SetOverrideText()`.
    pub fn set_override_text(&mut self, value: &str) {
        self.value_string = value.to_string();
    }

    /// `GetOverrideText()`.
    pub fn override_text(&self) -> &str {
        &self.value_string
    }

    /// `PCB_TEXT::GetTextTypeDescription (PCB_DIMENSION_BASE)`.
    pub fn text_type_description(&self, ctx: &BoardCtx<'_>) -> String {
        self.text.text_type_description(ctx)
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbDimension {
    fn invalidate_caches(&mut self) {
        self.text.invalidate_caches();
    }
}

impl BoardItem for PcbDimension {
    fn base(&self) -> &BoardItemBase {
        &self.text.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.text.base
    }
    fn type_(&self) -> KicadT {
        self.dimension_type()
    }
    fn class_name(&self) -> &'static str {
        self.dimension_class()
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbDimension::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbDimension::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbDimension::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbDimension::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbDimension::item_description(self, ctx, units, full)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbDimension::position(self, ctx)
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
        PcbDimension::transform_shape_to_polygon(
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
}

impl BoardItemMut for PcbDimension {
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbDimension::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbDimension::mirror(self, env, centre, dir)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbDimension::move_by(self, env, delta)
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
        PcbDimension::on_footprint_rescaled(
            self,
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate,
        )
    }
    fn on_footprint_transformed(&mut self, env: &MutEnv) {
        PcbDimension::on_footprint_transformed(self, env)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbDimension::rotate(self, env, centre, angle)
    }
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbDimension::set_position(self, env, pos)
    }
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        PcbDimension::style_from_settings(self, settings, check_side)
    }
}

impl EdaTextItem for PcbDimension {
    fn eda_text(&self) -> &EdaText {
        &self.text.text
    }
    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.text.text
    }
    fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbDimension::draw_rotation(self, ctx)
    }
    fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        PcbDimension::font_metrics(self, ctx)
    }
    fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbDimension::offset(self, env, delta)
    }
    fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        PcbDimension::set_text_angle(self, env, angle)
    }
    fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        PcbDimension::set_text_size(self, env, size, enforce_min)
    }
    fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        PcbDimension::set_text_thickness(self, env, width)
    }
    fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        PcbDimension::shown_text(self, ctx, context, depth)
    }
    fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        PcbDimension::text_angle(self, ctx)
    }
    fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbDimension::text_pos(self, ctx)
    }
    fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbDimension::text_size(self, ctx)
    }
    fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        PcbDimension::text_thickness(self, ctx)
    }
}

// ---- overrides and inherited PCB_TEXT bodies (package M12) -------------------------------

impl PcbDimension {
    /// `PCB_DIMENSION_BASE::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_DIMENSION_BASE::GetBoundingBox", ctx)
    }

    /// `PCB_DIMENSION_BASE::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!(
            "PCB_DIMENSION_BASE::GetEffectiveShape",
            ctx,
            layer,
            flash,
            usage
        )
    }

    /// `PCB_DIMENSION_BASE::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_DIMENSION_BASE::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_DIMENSION_BASE::HitTest`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!(
            "PCB_DIMENSION_BASE::HitTest",
            ctx,
            rect,
            contained,
            accuracy
        )
    }

    /// `PCB_DIMENSION_BASE::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_DIMENSION_BASE::GetItemDescription", ctx, units, full)
    }

    /// `PCB_DIMENSION_BASE::GetPosition`.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_DIMENSION_BASE::GetPosition", ctx)
    }

    /// `PCB_DIMENSION_BASE::TransformShapeToPolygon`.
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
            "PCB_DIMENSION_BASE::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_DIMENSION_BASE::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_DIMENSION_BASE::Flip", env, centre, dir)
    }

    /// `PCB_DIMENSION_BASE::Mirror`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_DIMENSION_BASE::Mirror", env, centre, dir)
    }

    /// `PCB_DIMENSION_BASE::Move`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        crate::port_pending!("PCB_DIMENSION_BASE::Move", env, delta)
    }

    /// `PCB_DIMENSION_BASE::OnFootprintRescaled`.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        ratio_x: f64,
        ratio_y: f64,
        linear_factor: f64,
        anchor: Vector2I,
        parent_rotate: EdaAngle,
    ) {
        crate::port_pending!(
            "PCB_DIMENSION_BASE::OnFootprintRescaled",
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate
        )
    }

    /// `PCB_DIMENSION_BASE::OnFootprintTransformed`.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_DIMENSION_BASE::OnFootprintTransformed", env)
    }

    /// `PCB_DIMENSION_BASE::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_DIMENSION_BASE::Rotate", env, centre, angle)
    }

    /// `PCB_DIMENSION_BASE::SetPosition`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("PCB_DIMENSION_BASE::SetPosition", env, pos)
    }

    /// `PCB_DIMENSION_BASE::StyleFromSettings`.
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        crate::port_pending!(
            "PCB_DIMENSION_BASE::StyleFromSettings",
            settings,
            check_side
        )
    }

    /// `PCB_TEXT::GetDrawRotation`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        self.text.draw_rotation(ctx)
    }

    /// `PCB_TEXT::getFontMetrics`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics {
        self.text.font_metrics(ctx)
    }

    /// `PCB_TEXT::Offset`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn offset(&mut self, env: &MutEnv, delta: Vector2I) {
        self.text.offset(env, delta)
    }

    /// `PCB_TEXT::SetTextAngle`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        self.text.set_text_angle(env, angle)
    }

    /// `PCB_TEXT::SetTextSize`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool) {
        self.text.set_text_size(env, size, enforce_min)
    }

    /// `PCB_TEXT::SetTextThickness`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn set_text_thickness(&mut self, env: &MutEnv, width: i32) {
        self.text.set_text_thickness(env, width)
    }

    /// `PCB_TEXT::GetShownText`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String {
        self.text.shown_text(ctx, context, depth)
    }

    /// `PCB_TEXT::GetTextAngle`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        self.text.text_angle(ctx)
    }

    /// `PCB_TEXT::GetTextPos`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.text.text_pos(ctx)
    }

    /// `PCB_TEXT::GetTextSize`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.text.text_size(ctx)
    }

    /// `PCB_TEXT::GetTextThickness`, inherited by `PCB_DIMENSION_BASE` (the base body reaches no virtual that `PCB_DIMENSION_BASE` overrides).
    pub fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.text.text_thickness(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_DIMENSION_BASE::PCB_DIMENSION_BASE` and the five `PCB_DIM_*` constructors
    /// (pcbnew/pcb_dimension.cpp).
    #[test]
    fn constructor_values() {
        let d = PcbDimension::default();
        assert_eq!(d.type_(), KicadT::PcbDimAlignedT);
        assert_eq!(d.text.base.layer, PcbLayerId::DwgsUser);
        assert_eq!(d.units, EdaUnits::Inch);
        assert_eq!(d.units_format, DimUnitsFormat::BareSuffix);
        assert_eq!(d.arrow_direction, DimArrowDirection::Outward);
        assert_eq!(d.precision, DimPrecision::X_XXXX);
        assert_eq!((d.line_thickness, d.arrow_length), (200_000, 1_270_000));
        assert_eq!(d.text_position, DimTextPosition::Outside);
        assert!(d.keep_text_aligned && !d.override_text_enabled && !d.auto_units);
        // ( int )( 1270000 * sin( 27.5 deg ) )
        let DimensionKind::Aligned(a) = &d.kind else {
            panic!()
        };
        assert_eq!((a.height, a.extension_height), (0, 586_420));

        let r = PcbDimension::of_type(KicadT::PcbDimRadialT).unwrap();
        assert_eq!(r.prefix, "R ");
        assert!(matches!(
            r.kind,
            DimensionKind::Radial {
                leader_length: 3_810_000
            }
        ));
        let l = PcbDimension::of_type(KicadT::PcbDimLeaderT).unwrap();
        assert_eq!(
            (l.value_string.as_str(), l.override_text_enabled),
            ("Leader", true)
        );
        assert!(!l.keep_text_aligned);
        let c = PcbDimension::of_type(KicadT::PcbDimCenterT).unwrap();
        assert_eq!(
            (c.units_format, c.override_text_enabled),
            (DimUnitsFormat::NoSuffix, true)
        );
        let o = PcbDimension::of_type(KicadT::PcbDimOrthogonalT).unwrap();
        assert_eq!(o.class_name(), "PCB_DIM_ORTHOGONAL");
        assert!(PcbDimension::of_type(KicadT::PcbTextT).is_none());

        let mut fx = rotated_flipped_scaled();
        let id = fx
            .child(ItemKind::Dimension)
            .typed::<PcbDimension>()
            .unwrap();
        let mut kiids = KiidGenerator::default();
        let mut leader = PcbDimension::new(&mut kiids, KicadT::PcbDimLeaderT).unwrap();
        leader.set_lib_points(Vector2I::new(1, 2), Vector2I::new(3, 4));
        fx.set_child(id, leader);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[id].lib_end(), Vector2I::new(3, 4));
        assert_eq!(fx.board[id].type_(), KicadT::PcbDimLeaderT);
    }
}
