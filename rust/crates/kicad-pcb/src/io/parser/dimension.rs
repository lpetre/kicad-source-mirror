// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of the dimension part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-10): `parseDIMENSION` (P:5766-6231), all five dimension types, the legacy
//! (pre-20200610) aligned form with its feature / crossbar / arrow lines, the embedded
//! `(gr_text ...)` and the final `Update()`.
//!
//! The dimension is built as a local value whose parent is set before parsing (the C++ passes
//! the parent to the constructor), so `SetStart` / `SetEnd` and the text setters see the parent
//! frame of [`Board::child_env`](crate::board::Board::child_env); `Update()` (and the
//! `updateText()` / `updateGeometry()` of `ChangeArrowDirection`) run against a bare
//! [`BoardCtx`] over the board being filled.

use kicad_core::eda_units::fetch_units_from_string;
use kicad_core::{EdaUnits, KicadT};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use crate::ctx::{BoardCtx, MutEnv};
use crate::item::ParentRef;
use crate::pcb_dimension::{
    DimArrowDirection, DimOrthogonalDir, DimPrecision, DimTextBorder, DimTextPosition,
    DimUnitsFormat, DimUnitsMode, DimensionKind, PcbDimension,
};
use crate::traits::BoardItemMut;

/// `static_cast<DIM_UNITS_MODE>( mode )` for `mode` in 0..=4 (the parser clamps to 4, one
/// past `AUTOMATIC`): `None` for 4, which `SetUnitsMode`'s switch ignores.
fn units_mode_from_int(mode: i32) -> Option<DimUnitsMode> {
    match mode {
        0 => Some(DimUnitsMode::Inch),
        1 => Some(DimUnitsMode::Mils),
        2 => Some(DimUnitsMode::Mm),
        3 => Some(DimUnitsMode::Automatic),
        _ => None,
    }
}

/// `static_cast<DIM_UNITS_FORMAT>( format )` for `format` in 0..=3. The C++ clamps to 3, one
/// past `PAREN_SUFFIX`, which `updateText()` renders like `NO_SUFFIX`; the Rust enum cannot hold
/// it and maps it to `NoSuffix` (deviation: re-saved as 0, `PORT_STATUS.md`).
fn units_format_from_int(format: i32) -> DimUnitsFormat {
    match format {
        1 => DimUnitsFormat::BareSuffix,
        2 => DimUnitsFormat::ParenSuffix,
        _ => DimUnitsFormat::NoSuffix,
    }
}

/// `static_cast<DIM_PRECISION>( parseInt( .. ) )`. The C++ does not clamp; values outside the
/// enumerators cannot be held by the Rust enum and are clamped into `X` ..= `V_VVVVV`
/// (deviation, `PORT_STATUS.md`).
fn precision_from_int(precision: i32) -> DimPrecision {
    match precision {
        i32::MIN..=0 => DimPrecision::X,
        1 => DimPrecision::X_X,
        2 => DimPrecision::X_XX,
        3 => DimPrecision::X_XXX,
        4 => DimPrecision::X_XXXX,
        5 => DimPrecision::X_XXXXX,
        6 => DimPrecision::V_VV,
        7 => DimPrecision::V_VVV,
        8 => DimPrecision::V_VVVV,
        _ => DimPrecision::V_VVVVV,
    }
}

/// `static_cast<DIM_TEXT_POSITION>( mode )` for `mode` in 0..=3. The C++ clamps to 3, one past
/// `MANUAL`; the Rust enum cannot hold it and maps it to `Manual` (deviation, `PORT_STATUS.md`).
fn text_position_from_int(mode: i32) -> DimTextPosition {
    match mode {
        0 => DimTextPosition::Outside,
        1 => DimTextPosition::Inline,
        _ => DimTextPosition::Manual,
    }
}

/// `static_cast<DIM_TEXT_BORDER>( textFrame )` for `textFrame` in 0..=3.
fn text_border_from_int(frame: i32) -> DimTextBorder {
    match frame {
        0 => DimTextBorder::None,
        1 => DimTextBorder::Rectangle,
        2 => DimTextBorder::Circle,
        _ => DimTextBorder::Roundrect,
    }
}

impl PcbParser {
    /// The `MutEnv` the setters of a not yet added dimension with parent `parent` see.
    fn dimension_env(&self, parent: ParentRef) -> MutEnv {
        match parent {
            ParentRef::None => MutEnv::detached(),
            _ => self.board.child_env(parent),
        }
    }

    /// `dim->SetStart( aPoint )`.
    fn dimension_set_start(&self, dim: &mut PcbDimension, point: kicad_geom::Vector2I) {
        let env = self.dimension_env(dim.text.base.parent);
        dim.set_start(&env, point);
    }

    /// `dim->SetEnd( aPoint )`.
    fn dimension_set_end(&self, dim: &mut PcbDimension, point: kicad_geom::Vector2I) {
        let env = self.dimension_env(dim.text.base.parent);
        dim.set_end(&env, point);
    }

    /// `PCB_DIMENSION_BASE::ChangeArrowDirection`: `SetArrowDirection`, `updateText()`,
    /// `updateGeometry()`.
    fn dimension_change_arrow_direction(
        &self,
        dim: &mut PcbDimension,
        direction: DimArrowDirection,
    ) {
        dim.arrow_direction = direction;
        let ctx = BoardCtx::bare(&self.board);
        dim.update_text(&ctx);
        dim.update_geometry(&ctx);
    }

    /// `parseXY( nullptr, nullptr )` twice inside a `(pts ...)` list and the two closing
    /// parentheses: the legacy arrow lines, no longer saved.
    fn skip_legacy_pts_line(&mut self) -> Result<(), IoError> {
        self.lexer.need_left()?;
        let token = self.lexer.next_tok()?;

        if token != T::T_pts {
            return Err(self.lexer.expecting(T::T_pts));
        }

        self.parse_xy()?;
        self.parse_xy()?;
        self.lexer.need_right()?;
        self.lexer.need_right()?;
        Ok(())
    }

    /// Port of `parseDIMENSION( BOARD_ITEM* aParent )`: `(dimension ...)` in the current and the
    /// legacy (6.0 and older) forms; ends with `Update()`. Call with `CurTok() == T_dimension`.
    ///
    /// Returns `Ok(None)` where the C++ returns `nullptr` (`wxCHECK_MSG` on an
    /// `extension_height` of a non-aligned or a `text_frame` of a non-leader dimension).
    ///
    /// # Errors
    ///
    /// `PARSE_ERROR` texts of the C++; an unknown `(type ...)` token, on which the C++
    /// dereferences a null pointer, is reported as "Cannot parse unknown dimension type ...".
    pub(crate) fn parse_dimension(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<PcbDimension>, IoError> {
        if self.lexer.cur_tok() != T::T_dimension {
            debug_assert!(
                false,
                "Cannot parse {} as DIMENSION.",
                self.lexer.token_text(self.lexer.cur_tok())
            );
            return Ok(None);
        }

        let mut locked = false;

        let mut token = self.lexer.next_tok()?;

        // Free 'locked' token from 6.0/7.0 formats
        if token == T::T_locked {
            locked = true;
            token = self.lexer.next_tok()?;
        }

        // skip value that used to be saved
        if token != T::T_LEFT {
            self.lexer.need_left()?;
        }

        token = self.lexer.next_tok()?;

        let mut is_legacy_dimension = false;
        let mut is_style_known = false;

        let mut dim: PcbDimension;

        // Old format
        if token == T::T_width {
            is_legacy_dimension = true;
            dim = self.new_dimension(KicadT::PcbDimAlignedT, parent);
            dim.line_thickness = self.parse_board_units_str("dimension width value")?;
            self.lexer.need_right()?;
        } else {
            if token != T::T_type {
                return Err(self.lexer.expecting(T::T_type));
            }

            let type_ = match self.lexer.next_tok()? {
                T::T_aligned => KicadT::PcbDimAlignedT,
                T::T_orthogonal => KicadT::PcbDimOrthogonalT,
                T::T_leader => KicadT::PcbDimLeaderT,
                T::T_center => KicadT::PcbDimCenterT,
                T::T_radial => KicadT::PcbDimRadialT,
                other => {
                    // wxFAIL_MSG, then a null dereference in the C++.
                    return Err(IoError::new(format!(
                        "Cannot parse unknown dimension type {}",
                        self.lexer.token_text(other)
                    )));
                }
            };

            dim = self.new_dimension(type_, parent);

            self.lexer.need_right()?;

            // Before parsing further, set default properites for old KiCad file
            // versions that didnt have these properties:
            dim.arrow_direction = DimArrowDirection::Outward;
        }

        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    let env = self.dimension_env(dim.text.base.parent);
                    BoardItemMut::set_layer(&mut dim, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    dim.text.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_gr_text => {
                    // In old pcb files, when parsing the text we do not yet know
                    // if the text is kept aligned or not, and its DIM_TEXT_POSITION option.
                    // Leave the text not aligned for now to read the text angle, and no
                    // constraint for DIM_TEXT_POSITION in this case.
                    // It will be set aligned (or not) later
                    let is_aligned = dim.keep_text_aligned;
                    let t_dim_pos = dim.text_position;

                    if !is_style_known {
                        dim.text_position = DimTextPosition::Manual;
                        dim.keep_text_aligned = false;
                    }

                    // parsePCB_TEXT( m_board, dim ): the parent passed is the board (so no
                    // footprint-text handling), the effects see the dimension's own parent.
                    let base_text = std::mem::take(&mut dim.text);
                    match self.parse_pcb_text(ParentRef::Board, Some(base_text))? {
                        Some(text) => dim.text = text,
                        None => return Ok(None),
                    }

                    if is_legacy_dimension {
                        let mut units = EdaUnits::Mm;

                        match fetch_units_from_string(dim.text.text.text()) {
                            Some(fetched) => units = fetched,
                            //Not determined => use automatic units
                            None => dim.auto_units = true,
                        }

                        dim.units = units;
                    }

                    if !is_style_known {
                        dim.keep_text_aligned = is_aligned;
                        dim.text_position = t_dim_pos;
                    }
                }
                // New format: feature points
                T::T_pts => {
                    let point = self.parse_xy()?;
                    self.dimension_set_start(&mut dim, point);
                    let point = self.parse_xy()?;
                    self.dimension_set_end(&mut dim, point);

                    self.lexer.need_right()?;
                }
                T::T_height => {
                    let height = self.parse_board_units_str("dimension height value")?;
                    self.lexer.need_right()?;

                    if let DimensionKind::Aligned(aligned)
                    | DimensionKind::Orthogonal { aligned, .. } = &mut dim.kind
                    {
                        aligned.height = height;
                    }
                }
                T::T_leader_length => {
                    let length = self.parse_board_units_str("leader length value")?;
                    self.lexer.need_right()?;

                    if let DimensionKind::Radial { leader_length } = &mut dim.kind {
                        *leader_length = length;
                    }
                }
                T::T_orientation => {
                    let orientation =
                        self.parse_int_expected("orthogonal dimension orientation")?;
                    self.lexer.need_right()?;

                    if let DimensionKind::Orthogonal {
                        orientation: dir, ..
                    } = &mut dim.kind
                    {
                        *dir = match orientation.clamp(0, 1) {
                            0 => DimOrthogonalDir::Horizontal,
                            _ => DimOrthogonalDir::Vertical,
                        };
                    }
                }
                T::T_format => self.parse_dimension_format(&mut dim, parent)?,
                T::T_style => {
                    is_style_known = true;

                    if !self.parse_dimension_style(&mut dim)? {
                        return Ok(None);
                    }
                }
                // Old format: feature1 stores a feature line.  We only care about the origin.
                T::T_feature1 => {
                    self.lexer.need_left()?;
                    token = self.lexer.next_tok()?;

                    if token != T::T_pts {
                        return Err(self.lexer.expecting(T::T_pts));
                    }

                    let point = self.parse_xy()?;
                    self.dimension_set_start(&mut dim, point);

                    self.parse_xy()?; // Ignore second point
                    self.lexer.need_right()?;
                    self.lexer.need_right()?;
                }
                // Old format: feature2 stores a feature line.  We only care about the end point.
                T::T_feature2 => {
                    self.lexer.need_left()?;
                    token = self.lexer.next_tok()?;

                    if token != T::T_pts {
                        return Err(self.lexer.expecting(T::T_pts));
                    }

                    let point = self.parse_xy()?;
                    self.dimension_set_end(&mut dim, point);

                    self.parse_xy()?; // Ignore second point

                    self.lexer.need_right()?;
                    self.lexer.need_right()?;
                }
                T::T_crossbar => {
                    self.lexer.need_left()?;
                    token = self.lexer.next_tok()?;

                    if token == T::T_pts {
                        // If we have a crossbar, we know we're an old aligned dim
                        // (the C++ static_casts unconditionally; UpdateHeight is a no-op on the
                        // non-aligned kinds here).

                        // Old style: calculate height from crossbar
                        let point1 = self.parse_xy()?;
                        let point2 = self.parse_xy()?;
                        let ctx = BoardCtx::bare(&self.board);
                        dim.update_height(&ctx, point2, point1); // Yes, backwards intentionally
                        self.lexer.need_right()?;
                    }

                    self.lexer.need_right()?;
                }
                // Arrow: no longer saved; no-op
                T::T_arrow1a | T::T_arrow1b | T::T_arrow2a | T::T_arrow2b => {
                    self.skip_legacy_pts_line()?;
                }
                // Handle (locked yes) from modern times
                T::T_locked => {
                    // Unsure if we ever wrote out (locked) for dimensions, so use maybeAbsent
                    // just in case
                    let is_locked = self.parse_maybe_absent_bool(true)?;
                    dim.text.base.is_locked = is_locked;
                }
                T::T_custom_property => {
                    self.parse_custom_property_item(&mut dim)?;
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "layer, tstamp, uuid, gr_text, feature1, feature2, crossbar, arrow1a, \
                         arrow1b, arrow2a, arrow2b, locked, or custom_property",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if locked {
            dim.text.base.is_locked = true;
        }

        let ctx = BoardCtx::bare(&self.board);
        dim.update(&ctx);

        Ok(Some(dim))
    }

    /// `std::make_unique<PCB_DIM_*>( aParent )`: a fresh UUID, parent set.
    fn new_dimension(&mut self, type_: KicadT, parent: ParentRef) -> PcbDimension {
        let mut dim = PcbDimension::new(&mut self.board.kiid_generator, type_)
            .expect("dimension type");
        dim.text.base.parent = parent;
        dim
    }

    /// The `case T_format:` block of `parseDIMENSION`: reads up to and including the `)` of
    /// `(format ...)`.
    fn parse_dimension_format(
        &mut self,
        dim: &mut PcbDimension,
        parent: ParentRef,
    ) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            match token {
                T::T_LEFT => {
                    token = self.lexer.next_tok()?;
                    continue;
                }
                T::T_prefix => {
                    self.lexer.need_symbol_or_number()?;
                    dim.prefix = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }
                T::T_suffix => {
                    self.lexer.need_symbol_or_number()?;
                    dim.suffix = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }
                T::T_units => {
                    let mode = self.parse_int_expected("dimension units mode")?;
                    let mode = 0.max(4.min(mode));

                    if let Some(mode) = units_mode_from_int(mode) {
                        // SetUnitsMode: GetBoard() ? GetBoard()->GetUserUnits() : MM
                        let user_units =
                            self.parent_reaches_board(parent).then_some(self.board.user_units);
                        dim.set_units_mode(mode, user_units);
                    }

                    self.lexer.need_right()?;
                }
                T::T_units_format => {
                    let format = self.parse_int_expected("dimension units format")?;
                    dim.units_format = units_format_from_int(format.clamp(0, 3));
                    self.lexer.need_right()?;
                }
                T::T_precision => {
                    let precision = self.parse_int_expected("dimension precision")?;
                    dim.precision = precision_from_int(precision);
                    self.lexer.need_right()?;
                }
                T::T_override_value => {
                    self.lexer.need_symbol_or_number()?;
                    dim.override_text_enabled = true;
                    let value = self.lexer.from_utf8();
                    dim.set_override_text(&value);
                    self.lexer.need_right()?;
                }
                T::T_suppress_zeroes => {
                    dim.suppress_zeroes = self.parse_maybe_absent_bool(true)?;
                }
                _ => {
                    // std::cerr << "Unknown format token: " ... (diagnostic only)
                    return Err(self.lexer.expecting_str(
                        "prefix, suffix, units, units_format, precision, override_value, \
                         suppress_zeroes",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `case T_style:` block of `parseDIMENSION`: reads up to and including the `)` of
    /// `(style ...)`. Returns `false` where the C++ `wxCHECK_MSG`s return `nullptr` from
    /// `parseDIMENSION`.
    fn parse_dimension_style(&mut self, dim: &mut PcbDimension) -> Result<bool, IoError> {
        // new format: default to keep text aligned off unless token is present
        dim.keep_text_aligned = false;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            match token {
                T::T_LEFT => {
                    token = self.lexer.next_tok()?;
                    continue;
                }
                T::T_thickness => {
                    dim.line_thickness =
                        self.parse_board_units_str("extension line thickness value")?;
                    self.lexer.need_right()?;
                }
                T::T_arrow_direction => {
                    token = self.lexer.next_tok()?;

                    if token == T::T_inward {
                        self.dimension_change_arrow_direction(dim, DimArrowDirection::Inward);
                    } else if token == T::T_outward {
                        self.dimension_change_arrow_direction(dim, DimArrowDirection::Outward);
                    } else {
                        return Err(self.lexer.expecting_str("inward or outward"));
                    }

                    self.lexer.need_right()?;
                }
                T::T_arrow_length => {
                    dim.arrow_length = self.parse_board_units_str("arrow length value")?;
                    self.lexer.need_right()?;
                }
                T::T_text_position_mode => {
                    let mode = self.parse_int_expected("text position mode")?;
                    let mode = 0.max(3.min(mode));
                    dim.text_position = text_position_from_int(mode);
                    self.lexer.need_right()?;
                }
                T::T_extension_height => {
                    // wxCHECK_MSG( aligned, nullptr, wxT( "Invalid extension_height token" ) )
                    if !matches!(
                        dim.kind,
                        DimensionKind::Aligned(_) | DimensionKind::Orthogonal { .. }
                    ) {
                        return Ok(false);
                    }

                    let height = self.parse_board_units_str("extension height value")?;

                    if let DimensionKind::Aligned(aligned)
                    | DimensionKind::Orthogonal { aligned, .. } = &mut dim.kind
                    {
                        aligned.extension_height = height;
                    }

                    self.lexer.need_right()?;
                }
                T::T_extension_offset => {
                    dim.extension_offset = self.parse_board_units_str("extension offset value")?;
                    self.lexer.need_right()?;
                }
                T::T_keep_text_aligned => {
                    dim.keep_text_aligned = self.parse_maybe_absent_bool(true)?;
                }
                T::T_text_frame => {
                    // wxCHECK_MSG( dim->Type() == PCB_DIM_LEADER_T, nullptr,
                    //              wxT( "Invalid text_frame token" ) )
                    if !matches!(dim.kind, DimensionKind::Leader { .. }) {
                        return Ok(false);
                    }

                    let text_frame = self.parse_int_expected("text frame mode")?;
                    let text_frame = text_frame.clamp(0, 3);

                    if let DimensionKind::Leader { text_border } = &mut dim.kind {
                        *text_border = text_border_from_int(text_frame);
                    }

                    self.lexer.need_right()?;
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "thickness, arrow_length, arrow_direction, text_position_mode, \
                         extension_height, extension_offset",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(true)
    }
}
