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
//! constructors and delegation shims (package M0c), the geometry, text and overrides (package
//! M12).
//!
//! `PcbDimension` is its own `EDA_TEXT` host (`PCB_DIMENSION_BASE : PCB_TEXT`,
//! `board-model.md` 6.2a); the `EDA_TEXT` virtuals that `PCB_TEXT` overrides and the dimension
//! inherits call the embedded `PcbText` (no dimension class overrides a virtual they reach).
//! Dropped: `m_inClearRenderCache`, `m_busy` (re-entrancy guards of the C++ virtual-call
//! chain).

use std::sync::{Arc, OnceLock};

use kicad_core::eda_units::{get_label, get_text, ui};
use kicad_core::lset::Lset;
use kicad_core::printf::format_f;
use kicad_core::text_vars::ResolutionContext;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{EdaDataType, EdaUnits, KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::cmath;
use kicad_geom::convert_basic_shapes_to_polygon::{
    transform_circle_to_polygon, transform_oval_to_polygon,
};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::circle::Circle;
use kicad_geom::{
    ANGLE_0, ANGLE_45, ANGLE_90, ANGLE_135, ANGLE_180, ANGLE_270, ANGLE_HORIZONTAL, ANGLE_VERTICAL,
    Box2I, EdaAngle, FULL_CIRCLE, FlipDirection, OptVector2I, Seg, Shape, ShapeCircle,
    ShapeCompound, ShapePolySet, ShapeRef, ShapeSegment, Vector2D, Vector2I, ki_round,
    mirror_point, rotate_point, rotate_point_around, sign,
};

use crate::board::Board;
use crate::board_design_settings::BoardDesignSettings;
use crate::board_item;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_text::{self, EdaText, EdaTextExt};
use crate::item::{BoardItemBase, SharedShape};
use crate::pcb_text::{self, PcbText};
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
        match board_item::parent_footprint(self, ctx) {
            Some(fp) => fp.transform().apply(self.lib_start),
            None => self.lib_start,
        }
    }

    /// `SetStart()`.
    pub fn set_start(&mut self, env: &MutEnv, point: Vector2I) {
        self.lib_start = match env.parent.as_ref() {
            Some(parent) => parent.transform.inverse_apply(point),
            None => point,
        };
    }

    /// `GetEnd()`: board frame.
    pub fn end(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        match board_item::parent_footprint(self, ctx) {
            Some(fp) => fp.transform().apply(self.lib_end),
            None => self.lib_end,
        }
    }

    /// `SetEnd()`.
    pub fn set_end(&mut self, env: &MutEnv, point: Vector2I) {
        self.lib_end = match env.parent.as_ref() {
            Some(parent) => parent.transform.inverse_apply(point),
            None => point,
        };
    }

    /// `Update()`: `updateGeometry()` + `updateText()` of the concrete class (reads the board
    /// for text variables and units: clone-edit-store, `board-model.md` 6.4).
    pub fn update(&mut self, ctx: &BoardCtx<'_>) {
        let env = self.own_env(ctx);
        self.update_with(ctx, &env);
    }

    /// `updateGeometry()` of the concrete class (pure virtual in `PCB_DIMENSION_BASE`).
    pub fn update_geometry(&mut self, ctx: &BoardCtx<'_>) {
        let env = self.own_env(ctx);
        self.update_geometry_with(ctx, &env);
    }

    /// `updateText()` of the concrete class.
    pub fn update_text(&mut self, ctx: &BoardCtx<'_>) {
        let env = self.own_env(ctx);
        self.update_text_with(ctx, &env);
    }

    /// `GetValueText()`: the measured value rendered with the precision and zero suppression,
    /// without units, prefix or suffix (`"%.<n>f"` of the value in `m_units`).
    pub fn value_text(&self) -> String {
        // localeconv()->decimal_point: the reference platform runs in the "C" locale.
        let sep = '.';

        let val = self.measured_value;
        let mut precision = self.precision as i32;

        if precision >= 6 {
            precision = match self.units {
                EdaUnits::Inch => precision - 4,
                EdaUnits::Mils => 0.max(precision - 7),
                EdaUnits::Mm => precision - 5,
                _ => precision - 4,
            };
        }

        let mut text = format_f(
            ui::to_user_unit(&PCB_IU_SCALE, self.units, val as f64),
            precision as usize,
        );

        if self.suppress_zeroes && precision > 0 {
            while text.ends_with('0') {
                text.pop();

                if text.ends_with('.') || text.ends_with(sep) {
                    text.pop();
                    break;
                }
            }
        }

        text
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

    /// `GetUnitsMode()`.
    pub fn units_mode(&self) -> DimUnitsMode {
        if self.auto_units {
            DimUnitsMode::Automatic
        } else {
            match self.units {
                EdaUnits::Mm => DimUnitsMode::Mm,
                EdaUnits::Mils => DimUnitsMode::Mils,
                _ => DimUnitsMode::Inch,
            }
        }
    }

    /// `SetUnitsMode()`. `board_user_units` is `GetBoard()->GetUserUnits()` (`None` without a
    /// board: millimetres), read for `AUTOMATIC` only.
    pub fn set_units_mode(&mut self, mode: DimUnitsMode, board_user_units: Option<EdaUnits>) {
        match mode {
            DimUnitsMode::Inch => {
                self.auto_units = false;
                self.units = EdaUnits::Inch;
            }
            DimUnitsMode::Mils => {
                self.auto_units = false;
                self.units = EdaUnits::Mils;
            }
            DimUnitsMode::Mm => {
                self.auto_units = false;
                self.units = EdaUnits::Mm;
            }
            DimUnitsMode::Automatic => {
                self.auto_units = true;
                self.units = board_user_units.unwrap_or(EdaUnits::Mm);
            }
        }
    }

    /// The board's user units as `SetUnitsMode` reads them (`GetBoard()`: the item's parent
    /// chain reaches the board).
    pub fn board_user_units(&self, ctx: &BoardCtx<'_>) -> Option<EdaUnits> {
        board_item::has_board(self, ctx).then_some(ctx.board.user_units)
    }

    /// `UpdateUnits()`: `SetUnitsMode( GetUnitsMode() )` and `Update()`.
    pub fn update_units(&mut self, ctx: &BoardCtx<'_>) {
        let units = self.board_user_units(ctx);
        self.set_units_mode(self.units_mode(), units);
        self.update(ctx);
    }

    /// `PCB_DIM_ALIGNED::UpdateHeight( aCrossbarStart, aCrossbarEnd )` (aligned and
    /// orthogonal dimensions; no-op on the others).
    pub fn update_height(
        &mut self,
        ctx: &BoardCtx<'_>,
        crossbar_start: Vector2I,
        crossbar_end: Vector2I,
    ) {
        let start = self.start(ctx);

        let Some(aligned) = self.aligned_mut() else {
            return;
        };

        let height = Vector2D::from(crossbar_start - start);
        let cross_bar = Vector2D::from(crossbar_end - crossbar_start);

        // m_height = ( -) height.EuclideanNorm(): double -> int truncation
        if height.cross(cross_bar) > 0.0 {
            aligned.height = -height.euclidean_norm() as i32;
        } else {
            aligned.height = height.euclidean_norm() as i32;
        }

        self.update(ctx);
    }

    /// `PCB_DIM_ALIGNED::GetAngle()`: the angle of the crossbar in radians.
    pub fn angle(&self, ctx: &BoardCtx<'_>) -> f64 {
        let delta = self.end(ctx) - self.start(ctx);

        cmath::atan2(delta.y as f64, delta.x as f64)
    }

    /// `PCB_DIM_RADIAL::GetKnee()`: the end of the leader line (radial dimensions; the end
    /// point for the other kinds, whose leader length is 0).
    pub fn knee(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        let leader_length = match self.kind {
            DimensionKind::Radial { leader_length } => leader_length,
            _ => 0,
        };
        let end = self.end(ctx);
        let radial = end - self.start(ctx);

        end + radial.resize(leader_length)
    }

    /// `PCB_DIM_ALIGNED::GetCrossbarStart()` / `GetCrossbarEnd()` (aligned and orthogonal).
    pub fn crossbar(&self) -> Option<(Vector2I, Vector2I)> {
        self.aligned().map(|a| (a.cross_bar_start, a.cross_bar_end))
    }

    /// `GetShapes()`: the shape cache built by `update()` (board frame).
    pub fn shapes(&self) -> &[SharedShape] {
        &self.shapes
    }

    // ---- private helpers --------------------------------------------------------------

    /// The `PCB_DIM_ALIGNED` members of an aligned or orthogonal dimension.
    fn aligned(&self) -> Option<&DimAligned> {
        match &self.kind {
            DimensionKind::Aligned(a) | DimensionKind::Orthogonal { aligned: a, .. } => Some(a),
            _ => None,
        }
    }

    fn aligned_mut(&mut self) -> Option<&mut DimAligned> {
        match &mut self.kind {
            DimensionKind::Aligned(a) | DimensionKind::Orthogonal { aligned: a, .. } => Some(a),
            _ => None,
        }
    }

    /// The environment the mutators of this dimension see (its own parent frame).
    fn own_env(&self, ctx: &BoardCtx<'_>) -> MutEnv {
        ctx.board.child_env(self.text.base.parent)
    }

    /// `GetStart()` as a mutator sees it.
    fn start_env(&self, env: &MutEnv) -> Vector2I {
        match env.parent.as_ref() {
            Some(parent) => parent.transform.apply(self.lib_start),
            None => self.lib_start,
        }
    }

    /// `GetEnd()` as a mutator sees it.
    fn end_env(&self, env: &MutEnv) -> Vector2I {
        match env.parent.as_ref() {
            Some(parent) => parent.transform.apply(self.lib_end),
            None => self.lib_end,
        }
    }

    /// The `Update()` that ends the C++ mutators taking no board context (`Move`, `Rotate`,
    /// `Mirror`, `StyleFromSettings`, `OnFootprintTransformed`). A board-level (or detached)
    /// dimension is updated against an empty board: exact unless its text holds variables
    /// that need the real board. A footprint child cannot see its footprint here; its derived
    /// state is refreshed by `update(ctx)` (`PORT_STATUS.md`, M12).
    fn update_after_edit(&mut self, env: &MutEnv) {
        static EMPTY: OnceLock<Board> = OnceLock::new();

        if env.parent.is_none() {
            let ctx = BoardCtx::bare(EMPTY.get_or_init(Board::new));
            self.update_with(&ctx, env);
        }
    }

    /// `Update()`: `updateGeometry()` (which calls `updateText()`).
    fn update_with(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        // Calls updateText internally
        self.update_geometry_with(ctx, env);
    }

    /// `updateGeometry()`, dispatched on the concrete class.
    fn update_geometry_with(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        match self.kind {
            DimensionKind::Aligned(_) => self.aligned_update_geometry(ctx, env),
            DimensionKind::Orthogonal { .. } => self.orthogonal_update_geometry(ctx, env),
            DimensionKind::Radial { .. } => self.radial_update_geometry(ctx, env),
            DimensionKind::Leader { .. } => self.leader_update_geometry(ctx, env),
            DimensionKind::Center => self.center_update_geometry(ctx, env),
        }
    }

    /// `updateText()`, dispatched on the concrete class.
    fn update_text_with(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        match self.kind {
            DimensionKind::Aligned(_) => self.aligned_update_text(ctx, env),
            DimensionKind::Orthogonal { .. } => self.orthogonal_update_text(ctx, env),
            DimensionKind::Radial { .. } => self.radial_update_text(ctx, env),
            // Our geometry is dependent on the size of the text, so just update the whole
            // shebang
            DimensionKind::Leader { .. } => self.leader_update_geometry(ctx, env),
            DimensionKind::Center => self.center_update_text(ctx, env),
        }
    }

    /// `PCB_DIMENSION_BASE::updateText()`: value (or override text), units suffix, prefix and
    /// suffix into the text.
    fn base_update_text(&mut self) {
        let mut text = if self.override_text_enabled {
            self.value_string.clone()
        } else {
            self.value_text()
        };

        match self.units_format {
            DimUnitsFormat::NoSuffix => {} // no units
            DimUnitsFormat::BareSuffix => {
                // normal
                text += &get_text(self.units, EdaDataType::Distance);
            }
            DimUnitsFormat::ParenSuffix => {
                // parenthetical
                text += " (";
                text += &get_label(self.units, EdaDataType::Distance);
                text += ")";
            }
        }

        let text = format!("{}{}{}", self.prefix, text, self.suffix);

        self.text.text.set_text(&text);
    }

    /// `PCB_DIMENSION_BASE::drawAnArrow( aStartPoint, anAngle, aLength )`.
    fn draw_an_arrow(&mut self, start_point: Vector2I, an_angle: EdaAngle, length: i32) {
        if length != 0 {
            let mut tail_end = Vector2I::new(length, 0);
            rotate_point(&mut tail_end, -an_angle);
            self.shapes
                .push(segment(start_point, start_point + tail_end));
        }

        let mut arrow_end_pos = Vector2I::new(self.arrow_length, 0);
        let mut arrow_end_neg = Vector2I::new(self.arrow_length, 0);

        rotate_point(&mut arrow_end_pos, -an_angle + arrow_angle());
        rotate_point(&mut arrow_end_neg, -an_angle - arrow_angle());

        self.shapes
            .push(segment(start_point, start_point + arrow_end_pos));
        self.shapes
            .push(segment(start_point, start_point + arrow_end_neg));
    }

    /// The text box, inflated by `( GetTextWidth() / 2, dy )` and rotated by the text angle
    /// about its centre, as the polygon the dimension lines are knocked out by. Returns the
    /// inflated box too.
    fn text_knockout_box(&self, ctx: &BoardCtx<'_>, dy: i32) -> (Box2I, ShapePolySet) {
        let mut text_box = self.text_box(ctx, None);
        text_box.inflate_xy(self.text_width(ctx) / 2, dy);

        let mut poly_box = ShapePolySet::new();
        poly_box.new_outline();
        poly_box.append_point(text_box.origin(), -1, -1);
        poly_box.append(text_box.origin().x, text_box.end().y, -1, -1, false);
        poly_box.append_point(text_box.end(), -1, -1);
        poly_box.append(text_box.end().x, text_box.origin().y, -1, -1, false);
        poly_box.rotate(self.text_angle(ctx), text_box.center());

        (text_box, poly_box)
    }

    // ---- PCB_DIM_ALIGNED --------------------------------------------------------------

    /// `PCB_DIM_ALIGNED::updateGeometry()`.
    fn aligned_update_geometry(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        self.shapes.clear();

        let start = self.start_env(env);
        let end = self.end_env(env);
        let dimension = end - start;

        self.measured_value = dimension.euclidean_norm();

        let a = self.aligned().cloned().expect("aligned dimension");

        let extension = if a.height > 0 {
            Vector2I::new(-dimension.y, dimension.x)
        } else {
            Vector2I::new(dimension.y, -dimension.x)
        };

        // Add extension lines
        let extension_height = a.height.abs() - self.extension_offset + a.extension_height;

        let mut ext_start = start;
        ext_start += extension.resize(self.extension_offset);

        self.shapes.push(segment(
            ext_start,
            ext_start + extension.resize(extension_height),
        ));

        ext_start = end;
        ext_start += extension.resize(self.extension_offset);

        self.shapes.push(segment(
            ext_start,
            ext_start + extension.resize(extension_height),
        ));

        // Add crossbar
        let cross_bar_distance = extension.resize(a.height) * sign(a.height);
        let cross_bar_start = start + cross_bar_distance;
        let cross_bar_end = end + cross_bar_distance;

        if let Some(a) = self.aligned_mut() {
            a.cross_bar_start = cross_bar_start;
            a.cross_bar_end = cross_bar_end;
        }

        // Update text after calculating crossbar position but before adding crossbar lines
        self.update_text_with(ctx, env);

        // Now that we have the text updated, we can determine how to draw the crossbar.
        // First we need to create an appropriate bounding polygon to collide with
        let pen = self.effective_text_pen_width(ctx, 0);
        let (_, poly_box) = self.text_knockout_box(ctx, -pen);

        // The ideal crossbar, if the text doesn't collide
        let crossbar = Seg::new(cross_bar_start, cross_bar_end);

        collect_knocked_out_segments(&poly_box, &crossbar, &mut self.shapes);

        let dimension_angle = EdaAngle::from_vector(dimension);

        if self.arrow_direction == DimArrowDirection::Inward {
            self.draw_an_arrow(
                cross_bar_start,
                dimension_angle + EdaAngle::from_degrees(180.0),
                self.arrow_length * INWARD_ARROW_LENGTH_TO_HEAD_RATIO,
            );
            self.draw_an_arrow(
                cross_bar_end,
                dimension_angle,
                self.arrow_length * INWARD_ARROW_LENGTH_TO_HEAD_RATIO,
            );
        } else {
            self.draw_an_arrow(cross_bar_start, dimension_angle, 0);
            self.draw_an_arrow(
                cross_bar_end,
                dimension_angle + EdaAngle::from_degrees(180.0),
                0,
            );
        }
    }

    /// `PCB_DIM_ALIGNED::updateText()`.
    fn aligned_update_text(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        let (cross_bar_start, cross_bar_end) = self.crossbar().expect("aligned dimension");
        let crossbar_center = (cross_bar_end - cross_bar_start) / 2;

        if self.text_position == DimTextPosition::Outside {
            let text_offset_distance =
                self.effective_text_pen_width(ctx, 0) + self.text_height(ctx);

            let rotation = if crossbar_center.x == 0 {
                ANGLE_90 * sign(-crossbar_center.y)
            } else if crossbar_center.x < 0 {
                -ANGLE_90
            } else {
                ANGLE_90
            };

            let mut text_offset = crossbar_center;
            rotate_point(&mut text_offset, rotation);
            text_offset = crossbar_center + text_offset.resize(text_offset_distance);

            self.set_text_pos(env, cross_bar_start + text_offset);
        } else if self.text_position == DimTextPosition::Inline {
            self.set_text_pos(env, cross_bar_start + crossbar_center);
        }

        if self.keep_text_aligned {
            let mut text_angle = FULL_CIRCLE - EdaAngle::from_vector(crossbar_center);
            text_angle.normalize();

            if text_angle > ANGLE_90 && text_angle <= ANGLE_270 {
                text_angle -= ANGLE_180;
            }

            self.set_text_angle(env, text_angle);
        }

        self.base_update_text();
    }

    // ---- PCB_DIM_ORTHOGONAL -----------------------------------------------------------

    /// `PCB_DIM_ORTHOGONAL::updateGeometry()`.
    fn orthogonal_update_geometry(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        self.shapes.clear();

        let start = self.start_env(env);
        let end = self.end_env(env);

        let DimensionKind::Orthogonal {
            aligned: a,
            orientation,
        } = self.kind.clone()
        else {
            unreachable!("orthogonal dimension")
        };
        let horizontal = orientation == DimOrthogonalDir::Horizontal;

        let measurement = if horizontal {
            end.x - start.x
        } else {
            end.y - start.y
        };
        self.measured_value = measurement.abs();

        let mut extension = if horizontal {
            Vector2I::new(0, a.height)
        } else {
            Vector2I::new(a.height, 0)
        };

        // Add first extension line
        let mut extension_height = a.height.abs() - self.extension_offset + a.extension_height;

        let mut ext_start = start;
        ext_start += extension.resize(self.extension_offset);

        self.shapes.push(segment(
            ext_start,
            ext_start + extension.resize(extension_height),
        ));

        // Add crossbar
        let cross_bar_distance = extension.resize(a.height) * sign(a.height);
        let cross_bar_start = start + cross_bar_distance;

        let cross_bar_end = if horizontal {
            Vector2I::new(end.x, cross_bar_start.y)
        } else {
            Vector2I::new(cross_bar_start.x, end.y)
        };

        if let Some(a) = self.aligned_mut() {
            a.cross_bar_start = cross_bar_start;
            a.cross_bar_end = cross_bar_end;
        }

        // Add second extension line (end to crossbar end)
        extension = if horizontal {
            Vector2I::new(0, end.y - cross_bar_end.y)
        } else {
            Vector2I::new(end.x - cross_bar_end.x, 0)
        };

        extension_height = extension.euclidean_norm() - self.extension_offset + a.extension_height;

        ext_start = cross_bar_end;
        ext_start -= extension.resize(a.extension_height);

        self.shapes.push(segment(
            ext_start,
            ext_start + extension.resize(extension_height),
        ));

        // Update text after calculating crossbar position but before adding crossbar lines
        self.update_text_with(ctx, env);

        // Now that we have the text updated, we can determine how to draw the crossbar.
        // First we need to create an appropriate bounding polygon to collide with
        let pen = self.effective_text_pen_width(ctx, 0);
        let (_, poly_box) = self.text_knockout_box(ctx, pen);

        // The ideal crossbar, if the text doesn't collide
        let crossbar = Seg::new(cross_bar_start, cross_bar_end);

        collect_knocked_out_segments(&poly_box, &crossbar, &mut self.shapes);

        let cross_bar_angle = EdaAngle::from_vector(cross_bar_end - cross_bar_start);

        if self.arrow_direction == DimArrowDirection::Inward {
            // Arrows with fixed length.
            self.draw_an_arrow(
                cross_bar_start,
                cross_bar_angle + EdaAngle::from_degrees(180.0),
                self.arrow_length * INWARD_ARROW_LENGTH_TO_HEAD_RATIO,
            );
            self.draw_an_arrow(
                cross_bar_end,
                cross_bar_angle,
                self.arrow_length * INWARD_ARROW_LENGTH_TO_HEAD_RATIO,
            );
        } else {
            self.draw_an_arrow(cross_bar_start, cross_bar_angle, 0);
            self.draw_an_arrow(
                cross_bar_end,
                cross_bar_angle + EdaAngle::from_degrees(180.0),
                0,
            );
        }
    }

    /// `PCB_DIM_ORTHOGONAL::updateText()` (ends with `PCB_DIM_ALIGNED::updateText()`).
    fn orthogonal_update_text(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        let (cross_bar_start, cross_bar_end) = self.crossbar().expect("orthogonal dimension");
        let horizontal = matches!(
            self.kind,
            DimensionKind::Orthogonal {
                orientation: DimOrthogonalDir::Horizontal,
                ..
            }
        );
        let crossbar_center = (cross_bar_end - cross_bar_start) / 2;

        if self.text_position == DimTextPosition::Outside {
            let text_offset_distance =
                self.effective_text_pen_width(ctx, 0) + self.text_height(ctx);

            let mut text_offset = Vector2I::new(0, 0);

            if horizontal {
                text_offset.y = -text_offset_distance;
            } else {
                text_offset.x = -text_offset_distance;
            }

            text_offset += crossbar_center;

            self.set_text_pos(env, cross_bar_start + text_offset);
        } else if self.text_position == DimTextPosition::Inline {
            self.set_text_pos(env, cross_bar_start + crossbar_center);
        }

        if self.keep_text_aligned {
            if crossbar_center.x.abs() > crossbar_center.y.abs() {
                self.set_text_angle(env, ANGLE_HORIZONTAL);
            } else {
                self.set_text_angle(env, ANGLE_VERTICAL);
            }
        }

        self.aligned_update_text(ctx, env);
    }

    // ---- PCB_DIM_LEADER ---------------------------------------------------------------

    /// `PCB_DIM_LEADER::updateGeometry()`.
    fn leader_update_geometry(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        self.shapes.clear();

        self.base_update_text();

        let text_border = match self.kind {
            DimensionKind::Leader { text_border } => text_border,
            _ => unreachable!("leader dimension"),
        };

        // Now that we have the text updated, we can determine how to draw the second line
        // First we need to create an appropriate bounding polygon to collide with
        let pen = self.effective_text_pen_width(ctx, 0);
        let (text_box, poly_box) = self.text_knockout_box(ctx, pen * 2);

        let board_start = self.start_env(env);
        let board_end = self.end_env(env);

        let first_line = board_end - board_start;
        let mut start = board_start;
        start += first_line.resize(self.extension_offset);

        let arrow_seg = Seg::new(board_start, board_end);
        let text_seg = Seg::new(board_end, self.text_pos(ctx));
        let mut arrow_seg_end: OptVector2I;
        let text_seg_end: OptVector2I;

        if text_border == DimTextBorder::Circle {
            let pen_width = pen as f64 / 2.0;
            let radius = (text_box.width() as f64 / 2.0) - pen_width;
            // CIRCLE( const VECTOR2I&, int ): double -> int truncation
            let circle = Circle::new(text_box.center(), radius as i32);

            arrow_seg_end = seg_circle_intersection(&circle, &arrow_seg, true);
            text_seg_end = seg_circle_intersection(&circle, &text_seg, true);
        } else {
            arrow_seg_end = seg_poly_intersection(&poly_box, &arrow_seg, true);
            text_seg_end = seg_poly_intersection(&poly_box, &text_seg, true);
        }

        if arrow_seg_end.is_none() {
            arrow_seg_end = Some(board_end);
        }

        let arrow_seg_end = arrow_seg_end.expect("set above");

        self.shapes.push(segment(start, arrow_seg_end));

        self.draw_an_arrow(start, EdaAngle::from_vector(first_line), 0);

        if !self.text.text.text().is_empty() {
            match text_border {
                DimTextBorder::Rectangle => {
                    for seg in poly_box.iterate_segments() {
                        self.shapes.push(Arc::new(ShapeSegment::from_seg(seg, 0)));
                    }
                }
                DimTextBorder::Circle => {
                    let pen_width = pen as f64 / 2.0;
                    let radius = (text_box.width() as f64 / 2.0) - pen_width;
                    // SHAPE_CIRCLE( const VECTOR2I&, int ): double -> int truncation
                    self.shapes
                        .push(Arc::new(ShapeCircle::new(text_box.center(), radius as i32)));
                }
                _ => {}
            }
        }

        if let Some(text_seg_end) = text_seg_end
            && arrow_seg_end == board_end
        {
            self.shapes.push(segment(board_end, text_seg_end));
        }
    }

    // ---- PCB_DIM_RADIAL ---------------------------------------------------------------

    /// `PCB_DIM_RADIAL::GetKnee()` as a mutator sees it.
    fn knee_env(&self, env: &MutEnv) -> Vector2I {
        let leader_length = match self.kind {
            DimensionKind::Radial { leader_length } => leader_length,
            _ => 0,
        };
        let end = self.end_env(env);
        let radial = end - self.start_env(env);

        end + radial.resize(leader_length)
    }

    /// `PCB_DIM_RADIAL::updateText()`.
    fn radial_update_text(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        if self.keep_text_aligned {
            let text_line = self.text_pos(ctx) - self.knee_env(env);
            let mut text_angle = FULL_CIRCLE - EdaAngle::from_vector(text_line);

            text_angle.normalize();

            if text_angle > ANGLE_90 && text_angle <= ANGLE_270 {
                text_angle -= ANGLE_180;
            }

            // Round to nearest degree
            text_angle = EdaAngle::from_degrees(ki_round(text_angle.as_degrees()) as f64);

            self.set_text_angle(env, text_angle);
        }

        self.base_update_text();
    }

    /// `PCB_DIM_RADIAL::updateGeometry()`.
    fn radial_update_geometry(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        self.shapes.clear();

        let board_start = self.start_env(env);
        let board_end = self.end_env(env);

        let center = board_start;
        let mut center_arm = Vector2I::new(0, self.arrow_length);

        self.shapes
            .push(segment(center - center_arm, center + center_arm));

        rotate_point(&mut center_arm, -ANGLE_90);

        self.shapes
            .push(segment(center - center_arm, center + center_arm));

        let radius = board_end - board_start;

        self.measured_value = radius.euclidean_norm();

        self.update_text_with(ctx, env);

        // Now that we have the text updated, we can determine how to draw the second line
        // First we need to create an appropriate bounding polygon to collide with
        let pen = self.effective_text_pen_width(ctx, 0);
        let (_, poly_box) = self.text_knockout_box(ctx, pen);

        let leader_length = match self.kind {
            DimensionKind::Radial { leader_length } => leader_length,
            _ => unreachable!("radial dimension"),
        };
        let mut radial = board_end - board_start;
        radial = radial.resize(leader_length);

        let arrow_seg = Seg::new(board_end, board_end + radial);
        let text_seg = Seg::new(arrow_seg.b, self.text_pos(ctx));

        collect_knocked_out_segments(&poly_box, &arrow_seg, &mut self.shapes);
        collect_knocked_out_segments(&poly_box, &text_seg, &mut self.shapes);

        self.draw_an_arrow(board_end, EdaAngle::from_vector(radial), 0);
    }

    // ---- PCB_DIM_CENTER ---------------------------------------------------------------

    /// `PCB_DIM_CENTER::updateText()`.
    fn center_update_text(&mut self, _ctx: &BoardCtx<'_>, env: &MutEnv) {
        // Even if PCB_DIM_CENTER has no text, we still need to update its text position
        // so GetTextPos() users get a valid value. Required at least for lasso hit-testing.
        let start = self.start_env(env);
        self.set_text_pos(env, start);

        self.base_update_text();
    }

    /// `PCB_DIM_CENTER::updateGeometry()`.
    fn center_update_geometry(&mut self, ctx: &BoardCtx<'_>, env: &MutEnv) {
        self.shapes.clear();

        let board_start = self.start_env(env);
        let board_end = self.end_env(env);
        let center = board_start;
        let mut arm = board_end - board_start;

        self.shapes.push(segment(center - arm, center + arm));

        rotate_point(&mut arm, -ANGLE_90);

        self.shapes.push(segment(center - arm, center + arm));

        self.update_text_with(ctx, env);
    }
}

/// `INWARD_ARROW_LENGTH_TO_HEAD_RATIO` (pcb_dimension.cpp).
const INWARD_ARROW_LENGTH_TO_HEAD_RATIO: i32 = 2;

/// `s_arrowAngle`.
fn arrow_angle() -> EdaAngle {
    EdaAngle::from_degrees(ARROW_ANGLE_DEGREES)
}

/// `new SHAPE_SEGMENT( aA, aB )` (width 0) as a shape of the cache.
fn segment(a: Vector2I, b: Vector2I) -> SharedShape {
    Arc::new(ShapeSegment::new(a, b, 0))
}

/// `segPolyIntersection( aPoly, aSeg, aStart )`: the point on `seg` colliding with `poly`
/// closest to its start (`seg.A`, or `seg.B` when `!start`), if one exists.
fn seg_poly_intersection(poly: &ShapePolySet, seg: &Seg, start: bool) -> OptVector2I {
    let start_pt = if start { seg.a } else { seg.b };
    let mut endpoint = if start { seg.b } else { seg.a };

    if poly.contains(start_pt, -1, 0, false) {
        return None;
    }

    for edge in poly.c_iterate_segments() {
        if let Some(intersection) = edge.intersect(seg)
            && (intersection - start_pt).squared_euclidean_norm()
                < (endpoint - start_pt).squared_euclidean_norm()
        {
            endpoint = intersection;
        }
    }

    if start_pt == endpoint {
        return None;
    }

    Some(endpoint)
}

/// `segCircleIntersection( CIRCLE& aCircle, SEG& aSeg, aStart )`. The circle is passed by
/// non-const reference, so `aCircle.Contains( start )` is the non-const overload: strictly
/// inside.
fn seg_circle_intersection(circle: &Circle, seg: &Seg, start: bool) -> OptVector2I {
    let start_pt = if start { seg.a } else { seg.b };
    let mut endpoint = if start { seg.b } else { seg.a };

    if circle.contains_inside(start_pt) {
        return None;
    }

    for intersection in circle.intersect_seg(seg) {
        if (intersection - start_pt).squared_euclidean_norm()
            < (endpoint - start_pt).squared_euclidean_norm()
        {
            endpoint = intersection;
        }
    }

    if start_pt == endpoint {
        return None;
    }

    Some(endpoint)
}

/// `CollectKnockedOutSegments( aPoly, aSeg, aSegmentsAfterKnockout )`: knock a polygon out
/// of a segment, adding 0, 1 or 2 segments depending on how the polygon intersects it.
fn collect_knocked_out_segments(poly: &ShapePolySet, seg: &Seg, out: &mut Vec<SharedShape>) {
    // Now we can draw 0, 1, or 2 crossbar lines depending on how the polygon collides
    let contains_a = poly.contains(seg.a, -1, 0, false);
    let contains_b = poly.contains(seg.b, -1, 0, false);

    let endpoint_a = seg_poly_intersection(poly, seg, true);
    let endpoint_b = seg_poly_intersection(poly, seg, false);

    if let Some(a) = endpoint_a {
        out.push(segment(seg.a, a));
    }

    if let Some(b) = endpoint_b {
        let mut can_add = true;

        if let Some(a) = endpoint_a
            && ((b == seg.a && a == seg.b) || (a == b && seg.a == seg.b))
        {
            can_add = false;
        }

        if can_add {
            out.push(segment(b, seg.b));
        }
    }

    if !contains_a && !contains_b && endpoint_a.is_none() && endpoint_b.is_none() {
        out.push(Arc::new(ShapeSegment::from_seg(*seg, 0)));
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
    /// `PCB_DIMENSION_BASE::GetBoundingBox` (`PCB_DIM_CENTER::GetBoundingBox` starts from the
    /// centre point instead of the text box).
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let mut b_box: Box2I;
        let (mut xmin, mut xmax, mut ymin, mut ymax): (i32, i32, i32, i32);

        if matches!(self.kind, DimensionKind::Center) {
            b_box = Box2I::default();
            let start = self.start(ctx);
            xmin = start.x;
            xmax = start.x;
            ymin = start.y;
            ymax = start.y;
        } else {
            b_box = self.text_box(ctx, None);
            xmin = b_box.x();
            xmax = b_box.right();
            ymin = b_box.y();
            ymax = b_box.bottom();
        }

        for shape in &self.shapes {
            let mut shape_box = shape.bbox(0);
            shape_box.inflate(self.line_thickness / 2);

            xmin = xmin.min(shape_box.origin().x);
            xmax = xmax.max(shape_box.end().x);
            ymin = ymin.min(shape_box.origin().y);
            ymax = ymax.max(shape_box.end().y);
        }

        b_box.set_x(xmin);
        b_box.set_y(ymin);
        b_box.set_width((xmax - xmin + 1) as i64);
        b_box.set_height((ymax - ymin + 1) as i64);

        b_box.normalize();

        b_box
    }

    /// `PCB_DIMENSION_BASE::GetEffectiveShape`: the text shape (`GetEffectiveTextShape()`)
    /// followed by the cached lines and circles.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        _layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let mut effective_shape = ShapeCompound::new();

        let text_shape = self.effective_text_shape(ctx, true, None, ANGLE_0);
        effective_shape.add_shape(text_shape.clone_shape());

        for shape in &self.shapes {
            effective_shape.add_shape(shape.clone_shape());
        }

        Arc::new(effective_shape)
    }

    /// `PCB_DIMENSION_BASE::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        if pcb_text::text_hit_test(self, ctx, pos, 0) {
            return true;
        }

        let dist_max = accuracy + (self.line_thickness / 2);

        // Locate SEGMENTS

        for shape in &self.shapes {
            if shape.collide_point(pos, dist_max, None, None) {
                return true;
            }
        }

        false
    }

    /// `PCB_DIMENSION_BASE::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut arect = *rect;
        arect.inflate(accuracy);

        let mut rect = self.bounding_box(ctx);

        if accuracy != 0 {
            rect.inflate(accuracy);
        }

        if contained {
            return arect.contains_box(&rect);
        }

        arect.intersects(&rect)
    }

    /// `PCB_DIMENSION_BASE::GetItemDescription` (untranslated).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        let content = if full {
            self.shown_text(ctx, ResolutionContext::ForGui, 0)
        } else {
            pcb_text::ellipsize_menu_text(self.text.text.text())
        };

        format!(
            "Dimension '{}' on {}",
            content,
            board_item::layer_name(self, ctx)
        )
    }

    /// `PCB_DIMENSION_BASE::GetPosition`: `GetStart()`.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.start(ctx)
    }

    /// `PCB_DIMENSION_BASE::TransformShapeToPolygon`: the cached circles and segments with
    /// the line thickness (the text is not included).
    pub fn transform_shape_to_polygon(
        &self,
        _ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        _layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        debug_assert!(
            !ignore_line_width,
            "IgnoreLineWidth has no meaning for dimensions."
        );

        for shape in &self.shapes {
            match shape.as_shape_ref() {
                ShapeRef::Circle(circle) => {
                    transform_circle_to_polygon(
                        buffer,
                        circle.get_center(),
                        circle.get_radius() + self.line_thickness / 2 + clearance,
                        error,
                        error_loc,
                        0,
                    );
                }
                ShapeRef::Segment(seg) => {
                    transform_oval_to_polygon(
                        buffer,
                        seg.get_seg().a,
                        seg.get_seg().b,
                        self.line_thickness + 2 * clearance,
                        error,
                        error_loc,
                        0,
                    );
                }
                _ => {
                    debug_assert!(
                        false,
                        "PCB_DIMENSION_BASE::TransformShapeToPolygon unknown shape type."
                    );
                }
            }
        }
    }

    /// `PCB_DIMENSION_BASE::Flip`: `Mirror()`, then the flipped layer.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror(env, centre, dir);

        let layer = env.layers.flip_layer(self.text.base.layer);
        self.set_layer(env, layer);
    }

    /// `PCB_DIM_ALIGNED::Mirror` / `PCB_DIM_ORTHOGONAL::Mirror` (the height) and
    /// `PCB_DIMENSION_BASE::Mirror`: points and text mirrored in the library frame of the
    /// parent footprint (board frame without one).
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        match &mut self.kind {
            DimensionKind::Aligned(a) => {
                a.height = -a.height;
            }
            // Only reverse the height if the height is aligned with the flip
            DimensionKind::Orthogonal {
                aligned,
                orientation,
            } if (*orientation == DimOrthogonalDir::Horizontal
                && dir == FlipDirection::TopBottom)
                || (*orientation == DimOrthogonalDir::Vertical
                    && dir == FlipDirection::LeftRight) =>
            {
                aligned.height = -aligned.height;
            }
            _ => {}
        }

        // Call this last for the Update()
        self.base_mirror(env, centre, dir);
    }

    /// `PCB_DIMENSION_BASE::Mirror`.
    fn base_mirror(&mut self, env: &MutEnv, axis_pos: Vector2I, dir: FlipDirection) {
        // IsSideSpecific(): the layer set part (MutEnv carries no layer types, as
        // PCB_TEXT::Flip; PORT_STATUS.md M10).
        let side_specific = Lset::side_specific_mask().contains(self.text.base.layer);

        if let Some(parent) = env.parent.as_ref() {
            let lib_axis = parent.transform.inverse_apply(axis_pos);

            let mirror_pt = |p: &mut Vector2I| {
                if dir == FlipDirection::LeftRight {
                    p.x = 2 * lib_axis.x - p.x;
                } else {
                    p.y = 2 * lib_axis.y - p.y;
                }
            };

            mirror_pt(&mut self.lib_start);
            mirror_pt(&mut self.lib_end);

            let mut lib_text_pos = self.text.text.lib_pos;
            mirror_pt(&mut lib_text_pos);
            self.text.set_lib_text_pos(lib_text_pos);

            let new_lib_angle = if dir == FlipDirection::LeftRight {
                ANGLE_180 - self.text.lib_text_angle()
            } else {
                -self.text.lib_text_angle()
            };
            self.text.set_lib_text_angle(new_lib_angle);

            if side_specific {
                let mirrored = !self.text.text.is_mirrored();
                self.text.text.set_mirrored(mirrored);
            }

            let angle = self.text.text_angle_env(env);
            self.set_text_angle(env, angle);
            self.update_after_edit(env);
            return;
        }

        let mut new_pos = eda_text::text_pos_env(self, env);

        mirror_point(&mut new_pos, axis_pos, dir);

        self.set_text_pos(env, new_pos);

        // invert angle
        let angle = -self.text.text_angle_env(env);
        self.set_text_angle(env, angle);

        mirror_point(&mut self.lib_start, axis_pos, dir);
        mirror_point(&mut self.lib_end, axis_pos, dir);

        if side_specific {
            let mirrored = !self.text.text.is_mirrored();
            self.text.text.set_mirrored(mirrored);
        }

        self.update_after_edit(env);
    }

    /// `PCB_DIMENSION_BASE::Move`: `PCB_TEXT::Offset`, then the points (a library-frame delta
    /// inside a footprint).
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        self.text.offset(env, delta);

        match env.parent.as_ref() {
            Some(parent) => {
                let xform = parent.transform;
                let lib_offset =
                    xform.inverse_apply(delta) - xform.inverse_apply(Vector2I::new(0, 0));
                self.lib_start += lib_offset;
                self.lib_end += lib_offset;
            }
            None => {
                self.lib_start += delta;
                self.lib_end += delta;
            }
        }

        self.update_after_edit(env);
    }

    /// `PCB_DIMENSION_BASE::OnFootprintRescaled`: `PCB_TEXT::OnFootprintRescaled` (which calls
    /// the virtual `OnFootprintTransformed()`, i.e. this class's), then
    /// `OnFootprintTransformed()` again.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        _ratio_x: f64,
        _ratio_y: f64,
        _linear_factor: f64,
        _anchor: Vector2I,
        _parent_rotate: EdaAngle,
    ) {
        self.on_footprint_transformed(env);
        self.on_footprint_transformed(env);
    }

    /// `PCB_DIMENSION_BASE::OnFootprintTransformed`: re-bake the board text angle, `Update()`,
    /// drop the text caches.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        let angle = self.text.text_angle_env(env);
        self.set_text_angle(env, angle);
        self.update_after_edit(env);
        self.text.text.clear_render_cache();
        self.text.text.clear_bounding_box_cache();
    }

    /// `PCB_DIMENSION_BASE::Rotate` (`PCB_DIM_ORTHOGONAL::Rotate` first adjusts the
    /// orientation and the height to the nearest quarter turn).
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        let mut angle = angle;

        if let DimensionKind::Orthogonal {
            aligned,
            orientation,
        } = &mut self.kind
        {
            // restrict angle to -179.9 to 180.0 degrees
            angle.normalize180();

            // adjust orientation and height to new angle
            // we can only handle the cases of -90, 0, 90, 180 degrees exactly;
            // in the other cases we will use the nearest 90 degree angle to
            // choose at least an approximate axis for the target orientation
            // In case of exactly 45 or 135 degrees, we will round towards zero for consistency
            if angle > ANGLE_45 && angle <= ANGLE_135 {
                // about 90 degree
                if *orientation == DimOrthogonalDir::Horizontal {
                    *orientation = DimOrthogonalDir::Vertical;
                } else {
                    *orientation = DimOrthogonalDir::Horizontal;
                    aligned.height = -aligned.height;
                }
            } else if angle < -ANGLE_45 && angle >= -ANGLE_135 {
                // about -90 degree
                if *orientation == DimOrthogonalDir::Horizontal {
                    *orientation = DimOrthogonalDir::Vertical;
                    aligned.height = -aligned.height;
                } else {
                    *orientation = DimOrthogonalDir::Horizontal;
                }
            } else if angle > ANGLE_135 || angle < -ANGLE_135 {
                // about 180 degree
                aligned.height = -aligned.height;
            }

            // this will update m_crossBarStart and m_crossbarEnd
        }

        let mut new_angle = self.text.text_angle_env(env) + angle;
        new_angle.normalize();
        self.set_text_angle(env, new_angle);

        let mut pt = eda_text::text_pos_env(self, env);
        rotate_point_around(&mut pt, centre, angle);
        self.set_text_pos(env, pt);

        let mut board_start = self.start_env(env);
        let mut board_end = self.end_env(env);
        rotate_point_around(&mut board_start, centre, angle);
        rotate_point_around(&mut board_end, centre, angle);

        match env.parent.as_ref() {
            Some(parent) => {
                self.lib_start = parent.transform.inverse_apply(board_start);
                self.lib_end = parent.transform.inverse_apply(board_end);
            }
            None => {
                self.lib_start = board_start;
                self.lib_end = board_end;
            }
        }

        self.update_after_edit(env);
    }

    /// `PCB_DIMENSION_BASE::SetPosition`: `SetStart( aPos )`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        self.set_start(env, pos);
    }

    /// `PCB_DIMENSION_BASE::StyleFromSettings`: the `PCB_TEXT` style, then the dimension
    /// defaults of the design settings, then `Update()`. Without a context, `AUTOMATIC` units
    /// resolve to millimetres (`GetBoard()->GetUserUnits()` is not reachable; `PORT_STATUS.md`).
    pub fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        self.text.style_from_settings(settings, check_side);

        self.line_thickness = settings.get_line_thickness(self.text.base.layer);
        self.set_units_mode(settings.dimension_units_mode, None);
        self.units_format = settings.dimension_units_format;
        self.precision = settings.dimension_precision;
        self.suppress_zeroes = settings.dimension_suppress_zeroes;
        self.text_position = settings.dimension_text_position;
        self.keep_text_aligned = settings.dimension_keep_text_aligned;

        // refresh text & geometry
        if self.text.base.parent.item().is_none() {
            self.update_after_edit(&MutEnv::detached());
        }
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

    /// `qa/tests/pcbnew/test_dimension_zero_suppression.cpp`:
    /// `DimensionZeroSuppressionPreservesIntegerMagnitude`.
    #[test]
    fn dimension_zero_suppression_preserves_integer_magnitude() {
        let mut dimension = PcbDimension::of_type(KicadT::PcbDimAlignedT).unwrap();
        dimension.units = EdaUnits::Mm;
        dimension.suppress_zeroes = true;

        for precision in [DimPrecision::X, DimPrecision::X_XXX] {
            dimension.precision = precision;

            for value in [0, 10, 100, 1000] {
                dimension.measured_value = PCB_IU_SCALE.mm_to_iu(value as f64);
                assert_eq!(dimension.value_text(), format!("{value}"));
            }
        }

        dimension.precision = DimPrecision::X_XXX;
        dimension.measured_value = PCB_IU_SCALE.mm_to_iu(10.25);
        assert_eq!(dimension.value_text(), "10.25");
        dimension.suppress_zeroes = false;
        assert_eq!(dimension.value_text(), "10.250");

        dimension.units = EdaUnits::Mils;
        dimension.precision = DimPrecision::V_VV;
        dimension.suppress_zeroes = true;
        dimension.measured_value = PCB_IU_SCALE.mils_to_iu(10);
        assert_eq!(dimension.value_text(), "10");
    }

    /// `GetUnitsMode` / `SetUnitsMode` round trip.
    #[test]
    fn units_mode() {
        let mut d = PcbDimension::default();
        assert_eq!(d.units_mode(), DimUnitsMode::Inch);
        d.set_units_mode(DimUnitsMode::Mils, None);
        assert_eq!(
            (d.units, d.units_mode()),
            (EdaUnits::Mils, DimUnitsMode::Mils)
        );
        d.set_units_mode(DimUnitsMode::Automatic, Some(EdaUnits::Inch));
        assert_eq!(
            (d.units, d.units_mode()),
            (EdaUnits::Inch, DimUnitsMode::Automatic)
        );
        d.set_units_mode(DimUnitsMode::Automatic, None);
        assert_eq!(d.units, EdaUnits::Mm);
    }

    // ---- qa/tests/pcbnew/test_board_item.cpp (the five PCB_DIM_* rows) ----

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

    /// `CompareItems`: position and bounding box edges.
    fn compare_items(ctx: &BoardCtx<'_>, a: &PcbDimension, b: &PcbDimension) {
        assert_eq!(a.position(ctx), b.position(ctx));
        let (ba, bb) = (a.bounding_box(ctx), b.bounding_box(ctx));
        assert_eq!(
            (ba.top(), ba.left(), ba.bottom(), ba.right()),
            (bb.top(), bb.left(), bb.bottom(), bb.right())
        );
    }

    /// `Move`, `Rotate`, `FlipLeftRight`, `FlipUpDown` of `test_board_item.cpp` for the
    /// dimensions `Instantiate()` makes (parent: the board): move there and back, four
    /// quarter turns and two flips are identities.
    #[test]
    fn board_item_move_rotate_flip_are_identities() {
        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        let env = MutEnv::detached();

        for type_ in [
            KicadT::PcbDimAlignedT,
            KicadT::PcbDimLeaderT,
            KicadT::PcbDimCenterT,
            KicadT::PcbDimRadialT,
            KicadT::PcbDimOrthogonalT,
        ] {
            let mut item = PcbDimension::of_type(type_).unwrap();
            item.text.base.parent = crate::item::ParentRef::Board;

            iterate_positions(|pos, r| {
                let mut original = item.clone();
                original.set_position(&env, pos);
                // FIXME: Update() has to be called after SetPosition() to update dimension
                // shapes.
                original.update(&ctx);

                // Move to a point, then go back.
                let mut moved = original.clone();
                let before = moved.position(&ctx);
                moved.move_by(&env, r);
                assert_eq!(moved.position(&ctx), before + r);
                moved.move_by(&env, -r);
                compare_items(&ctx, &moved, &original);

                // Four equivalent 90 degree rotations are an identity.
                let mut rotated = original.clone();

                for _ in 0..4 {
                    rotated.rotate(&env, r, EdaAngle::from_degrees(90.0));
                }

                compare_items(&ctx, &rotated, &original);

                // Two equivalent flips are an identity.
                for dir in [FlipDirection::LeftRight, FlipDirection::TopBottom] {
                    let mut flipped = original.clone();
                    flipped.flip(&env, r, dir);
                    flipped.flip(&env, r, dir);
                    compare_items(&ctx, &flipped, &original);
                    assert_eq!(flipped.text.base.layer, original.text.base.layer);
                }
            });
        }
    }

    /// `qa/tests/pcbnew/test_group_mirror.cpp`: `GroupWithDimensionStillMirrors`, the
    /// dimension part (the group cascade is `PCB_GROUP::Mirror`, package M23).
    #[test]
    fn dimension_mirrors_across_axis() {
        let env = MutEnv::detached();
        let mut dim = PcbDimension::of_type(KicadT::PcbDimAlignedT).unwrap();
        dim.text.base.parent = crate::item::ParentRef::Board;
        dim.set_start(&env, Vector2I::new(1_000_000, 0));
        dim.set_end(&env, Vector2I::new(5_000_000, 0));

        dim.mirror(&env, Vector2I::new(0, 0), FlipDirection::LeftRight);

        assert_eq!(dim.lib_start(), Vector2I::new(-1_000_000, 0));
        assert_eq!(dim.lib_end(), Vector2I::new(-5_000_000, 0));
    }

    /// `FooBar` / `X_XX` -> `FOO_BAR` / `X_XX` (the dump's enum names).
    fn upper_snake(name: &str) -> String {
        let mut out = String::new();

        let mut prev_lower = false;

        for c in name.chars() {
            if c.is_ascii_uppercase() && prev_lower {
                out.push('_');
            }

            prev_lower = c.is_ascii_lowercase();
            out.push(c.to_ascii_uppercase());
        }

        out
    }

    /// The enum value whose dump name is `name`.
    fn enum_name<E: std::fmt::Debug + Copy>(all: &[E], name: &str) -> E {
        *all.iter()
            .find(|e| upper_snake(&format!("{e:?}")) == name)
            .unwrap_or_else(|| panic!("unknown enum value {name}"))
    }

    fn vec2(s: &str) -> Vector2I {
        let (x, y) = s.split_once(',').unwrap();
        Vector2I::new(x.parse().unwrap(), y.parse().unwrap())
    }

    fn unquote(s: &str) -> String {
        s.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap()
            .to_string()
    }

    /// A dimension built from the `in|` members of the golden (what the parser stores), with
    /// the text attributes set through the parser's setters in the dimension's frame.
    fn dimension_from_inputs(
        input: &std::collections::BTreeMap<String, String>,
        env: &MutEnv,
    ) -> PcbDimension {
        use kicad_core::{GrTextHAlignT, GrTextVAlignT};

        let get = |k: &str| input.get(k).map(String::as_str);
        let type_ = match get("type").unwrap() {
            "PCB_DIM_ALIGNED_T" => KicadT::PcbDimAlignedT,
            "PCB_DIM_ORTHOGONAL_T" => KicadT::PcbDimOrthogonalT,
            "PCB_DIM_RADIAL_T" => KicadT::PcbDimRadialT,
            "PCB_DIM_LEADER_T" => KicadT::PcbDimLeaderT,
            _ => KicadT::PcbDimCenterT,
        };
        let mut d = PcbDimension::of_type(type_).unwrap();
        let layer = (0..kicad_core::PCB_LAYER_ID_COUNT)
            .find(|l| kicad_core::lset::Lset::name(PcbLayerId(*l)) == get("layer").unwrap())
            .map(PcbLayerId)
            .unwrap();
        d.text.base.layer = layer;
        d.set_lib_points(
            vec2(get("lib_start").unwrap()),
            vec2(get("lib_end").unwrap()),
        );
        d.override_text_enabled = get("override_text_enabled") == Some("true");
        d.value_string = unquote(get("override_text").unwrap());
        d.prefix = unquote(get("prefix").unwrap());
        d.suffix = unquote(get("suffix").unwrap());
        d.arrow_direction = enum_name(
            &[DimArrowDirection::Inward, DimArrowDirection::Outward],
            get("arrow_direction").unwrap(),
        );
        d.units = enum_name(
            &[EdaUnits::Inch, EdaUnits::Mils, EdaUnits::Mm],
            get("units").unwrap(),
        );
        d.auto_units = get("units_mode") == Some("AUTOMATIC");
        d.units_format = enum_name(
            &[
                DimUnitsFormat::NoSuffix,
                DimUnitsFormat::BareSuffix,
                DimUnitsFormat::ParenSuffix,
            ],
            get("units_format").unwrap(),
        );
        d.precision = enum_name(
            &[
                DimPrecision::X,
                DimPrecision::X_X,
                DimPrecision::X_XX,
                DimPrecision::X_XXX,
                DimPrecision::X_XXXX,
                DimPrecision::X_XXXXX,
                DimPrecision::V_VV,
                DimPrecision::V_VVV,
                DimPrecision::V_VVVV,
                DimPrecision::V_VVVVV,
            ],
            get("precision").unwrap(),
        );
        d.suppress_zeroes = get("suppress_zeroes") == Some("true");
        d.keep_text_aligned = get("keep_text_aligned") == Some("true");
        d.text_position = enum_name(
            &[
                DimTextPosition::Outside,
                DimTextPosition::Inline,
                DimTextPosition::Manual,
            ],
            get("text_position_mode").unwrap(),
        );
        d.arrow_length = get("arrow_length").unwrap().parse().unwrap();
        d.extension_offset = get("extension_offset").unwrap().parse().unwrap();
        d.line_thickness = get("line_thickness").unwrap().parse().unwrap();

        match &mut d.kind {
            DimensionKind::Aligned(a) | DimensionKind::Orthogonal { aligned: a, .. } => {
                a.height = get("height").unwrap().parse().unwrap();
                a.extension_height = get("extension_height").unwrap().parse().unwrap();
            }
            DimensionKind::Radial { leader_length } => {
                *leader_length = get("leader_length").unwrap().parse().unwrap();
            }
            DimensionKind::Leader { text_border } => {
                *text_border = enum_name(
                    &[
                        DimTextBorder::None,
                        DimTextBorder::Rectangle,
                        DimTextBorder::Circle,
                        DimTextBorder::Roundrect,
                    ],
                    get("text_border").unwrap(),
                );
            }
            DimensionKind::Center => {}
        }

        if let DimensionKind::Orthogonal { orientation, .. } = &mut d.kind {
            *orientation = enum_name(
                &[DimOrthogonalDir::Horizontal, DimOrthogonalDir::Vertical],
                get("orientation").unwrap(),
            );
        }

        // The parser's SetTextSize / SetTextThickness: board-frame values.
        d.set_text_size(env, vec2(get("text_size").unwrap()), true);
        d.set_text_thickness(env, get("text_thickness").unwrap().parse().unwrap());
        let et = &mut d.text.text;
        et.set_italic_flag(get("italic") == Some("true"));
        et.set_bold_flag(get("bold") == Some("true"));
        et.set_visible(get("visible") == Some("true"));
        et.set_mirrored(get("mirrored") == Some("true"));
        et.set_multiline_allowed(get("multiline") == Some("true"));
        et.set_keep_upright(get("keep_upright") == Some("true"));
        et.set_line_spacing(get("line_spacing").unwrap().parse().unwrap());
        et.set_horiz_justify(enum_name(
            &[
                GrTextHAlignT::Left,
                GrTextHAlignT::Center,
                GrTextHAlignT::Right,
            ],
            get("h_justify")
                .unwrap()
                .trim_start_matches("GR_TEXT_H_ALIGN_"),
        ));
        et.set_vert_justify(enum_name(
            &[
                GrTextVAlignT::Top,
                GrTextVAlignT::Center,
                GrTextVAlignT::Bottom,
            ],
            get("v_justify")
                .unwrap()
                .trim_start_matches("GR_TEXT_V_ALIGN_"),
        ));

        d.set_text_pos(env, vec2(get("text_pos").unwrap()));
        let angle: f64 = get("text_angle").unwrap().parse().unwrap();
        d.set_text_angle(env, EdaAngle::from_degrees(angle));
        d
    }

    /// The board of the M12 oracle (`tests/golden/m12_dimensions.py`): every dimension of the
    /// golden rebuilt from its stored members and `update()`d against the board
    /// (clone-edit-store, as the loader's `Update()`), plus the reference lines.
    struct OracleBoard {
        fx: crate::test_support::FrameFixture,
        ids: Vec<crate::ids::DimensionId>,
        keys: Vec<String>,
        expected: Vec<(String, String)>,
    }

    fn oracle_board() -> OracleBoard {
        use std::collections::BTreeMap;

        use crate::board::AddMode;
        use crate::test_support::{board_with_footprint, rotated_scaled_transform};

        let golden = include_str!("../tests/golden/m12_dimensions.txt");
        let mut inputs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut keys: Vec<String> = Vec::new();
        let mut expected: Vec<(String, String)> = Vec::new();

        for line in golden.lines().filter(|l| !l.starts_with('#')) {
            let (head, value) = line.split_once('=').unwrap();
            let mut parts = head.splitn(3, '|');
            let (section, key, field) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );

            if section == "in" {
                if !inputs.contains_key(key) {
                    keys.push(key.to_string());
                }

                inputs
                    .entry(key.to_string())
                    .or_default()
                    .insert(field.to_string(), value.to_string());
            } else {
                expected.push((head.to_string(), value.to_string()));
            }
        }

        let mut fx = board_with_footprint(rotated_scaled_transform(), false);
        let mut ids = Vec::new();

        for key in &keys {
            let input = &inputs[key];
            let in_fp = input["parent"] != "board";
            let parent = if in_fp {
                crate::item::ParentRef::Item(fx.footprint.into())
            } else {
                crate::item::ParentRef::Board
            };
            let env = fx.board.child_env(parent);
            let d = dimension_from_inputs(input, &env);

            let id = if in_fp {
                fx.add_child(d, |fp| &mut fp.drawings)
            } else {
                fx.board.add_dimension(d, AddMode::Append)
            };

            fx.board.dimensions.get_mut(id).unwrap().text.base.uuid = Kiid::parse(key).unwrap();
            ids.push(id);
        }

        for id in &ids {
            let mut d = fx.board[*id].clone();
            d.update(&BoardCtx::bare(&fx.board));
            *fx.board.dimensions.get_mut(*id).unwrap() = d;
        }

        OracleBoard {
            fx,
            ids,
            keys,
            expected,
        }
    }

    /// `SHAPE::Format`-like text of the dump (`v_shape`) for the two shape types of the cache.
    fn shape_text(shape: &SharedShape) -> String {
        match shape.as_shape_ref() {
            ShapeRef::Segment(s) => format!(
                "SH_SEGMENT(a={},{} b={},{} w={})",
                s.get_seg().a.x,
                s.get_seg().a.y,
                s.get_seg().b.x,
                s.get_seg().b.y,
                s.get_width()
            ),
            ShapeRef::Circle(c) => format!(
                "SH_CIRCLE(c={},{} r={})",
                c.get_center().x,
                c.get_center().y,
                c.get_radius()
            ),
            _ => "?".to_string(),
        }
    }

    /// Oracle without the dump feature: value text, measured value, units mode, text, text
    /// position and angle, crossbar, `GetAngle`, `GetKnee`, every cached shape, `GetTextBox`,
    /// `GetBoundingBox` and `GetPosition` of the 31 oracle dimensions (five corpus boards and `tests/golden/m12_dimensions.kicad_pcb`:
    /// all five classes, both arrow directions, the three text position modes, all unit
    /// formats, zero suppression, the `V_*` precisions, the three leader frames, mirrored bold
    /// text, `${LAYER}`, two dimensions in a rotated, non-uniformly scaled footprint).
    #[test]
    fn oracle_dimensions_update() {
        use std::collections::BTreeMap;

        let ob = oracle_board();
        let ctx = BoardCtx::bare(&ob.fx.board);
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        let mut angles: BTreeMap<String, f64> = BTreeMap::new();

        for (id, key) in ob.ids.iter().zip(&ob.keys) {
            let d = &ob.fx.board[*id];
            let mut put = |section: &str, field: &str, value: String| {
                values.insert(format!("{section}|{key}|{field}"), value);
            };
            let v = |p: Vector2I| format!("{},{}", p.x, p.y);
            let b = |b: Box2I| format!("{},{},{},{}", b.x(), b.y(), b.width(), b.height());

            put("item", "position", v(d.position(&ctx)));
            put("dim", "start", v(d.start(&ctx)));
            put("dim", "end", v(d.end(&ctx)));
            put("dim", "measured_value", d.measured_value.to_string());
            put("dim", "value_text", format!("\"{}\"", d.value_text()));
            put("dim", "shape_count", d.shapes.len().to_string());
            put(
                "dim",
                "units_mode",
                upper_snake(&format!("{:?}", d.units_mode())),
            );

            match d.kind {
                DimensionKind::Aligned(_) | DimensionKind::Orthogonal { .. } => {
                    angles.insert(format!("dim|{key}|angle"), d.angle(&ctx));
                }
                DimensionKind::Radial { .. } => put("dim", "knee", v(d.knee(&ctx))),
                _ => {}
            }

            for (i, s) in d.shapes.iter().enumerate() {
                put("dim", &format!("shapes.{i}"), shape_text(s));
            }

            if let Some((a, e)) = d.crossbar() {
                put("dim", "crossbar", format!("{} {}", v(a), v(e)));
            }

            put("text", "text", format!("\"{}\"", d.text.text.text()));
            put("text", "text_pos", v(d.text_pos(&ctx)));
            put("text", "text_box", b(d.text_box(&ctx, None)));
            put("bbox", "bbox", b(d.bounding_box(&ctx)));
            angles.insert(
                format!("text|{key}|text_angle"),
                d.text_angle(&ctx).as_degrees(),
            );
        }

        let mut failures = Vec::new();
        let mut compared = 0;

        for (head, value) in &ob.expected {
            if let Some(a) = angles.get(head) {
                compared += 1;

                if *a != value.parse::<f64>().unwrap() {
                    failures.push(format!("{head}: ref {value}, rust {a}"));
                }
            } else if let Some(v) = values.get(head) {
                compared += 1;

                if v != value {
                    failures.push(format!("{head}: ref {value}, rust {v}"));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "{} of {compared} fields differ:\n{}",
            failures.len(),
            failures.join("\n")
        );
        assert!(compared >= 616, "only {compared} fields compared");
    }

    /// The full reference dump of the oracle dimensions (`--features dump`): the fields above
    /// plus `GetEffectiveShape`, `GetEffectiveTextShape`, the text polygons and the
    /// `TransformShapeToPolygon` polygons, through the Rust `pcb-dump`.
    #[cfg(feature = "dump")]
    #[test]
    fn oracle_dimensions_dump() {
        use std::collections::{BTreeMap, BTreeSet};

        use crate::dump::{DumpOptions, dump_board, fnv1a64};

        let ob = oracle_board();
        let opts = DumpOptions {
            sections: ["item", "bbox", "dim", "text"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            hash: false,
            file_uuids: Some(ob.keys.iter().cloned().collect::<BTreeSet<_>>()),
        };
        let rust = dump_board(&ob.fx.board, &opts);
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        let mut pending: BTreeMap<String, String> = BTreeMap::new();

        for line in rust.lines() {
            let Some((head, value)) = line.split_once('=') else {
                continue;
            };

            if let Some(rest) = head.strip_prefix("pending|") {
                pending.insert(rest.to_string(), value.to_string());
                continue;
            }

            let field = head.rsplit('|').next().unwrap();
            let hashed = ["tpoly.", "text_polys.", "text_shape", "effective_shape"]
                .iter()
                .any(|p| field.starts_with(p));
            let value = if hashed {
                format!("#{:016x}", fnv1a64(value.as_bytes()))
            } else {
                value.to_string()
            };
            values.insert(head.to_string(), value);
        }

        let mut failures = Vec::new();
        let mut pending_fields = BTreeSet::new();

        for (head, value) in &ob.expected {
            match values.get(head) {
                Some(v) if v == value => {}
                Some(v) => failures.push(format!("mismatch {head}\n  ref:  {value}\n  rust: {v}")),
                None => match pending.get(head) {
                    Some(p) => {
                        pending_fields.insert(format!("{}={p}", head.rsplit('|').next().unwrap()));
                    }
                    None => failures.push(format!("missing {head} (ref {value})")),
                },
            }
        }

        assert!(
            failures.is_empty(),
            "{} of {} fields differ:\n{}",
            failures.len(),
            ob.expected.len(),
            failures.join("\n")
        );
        eprintln!("pending (other packages): {pending_fields:?}");
    }
}
