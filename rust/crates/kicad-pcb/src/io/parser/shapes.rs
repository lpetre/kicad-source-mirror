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

//! Port of the graphic-shape part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-03): `parseLineEnding` (P:3886-3937), `parsePCB_SHAPE` (P:3940-4456), and the
//! `STROKE_PARAMS_PARSER` hookup (`common/stroke_params.cpp`, run on the same input through
//! `DsnLexer::with_sub_lexer`, the C++ `SyncLineReaderWith` pair).
//!
//! The shape is built as a local `PCB_SHAPE` whose parent is `aParent` (the board, the
//! footprint being parsed, or none for pad primitives). The C++ calls the `PCB_SHAPE` overrides
//! of the `EDA_SHAPE` setters (they keep the library-frame copy in sync); the port calls the
//! same overrides on [`PcbShape`] with the environment of the parent (`Board::child_env`), and
//! the `EDA_SHAPE` body directly where `PCB_SHAPE` has no override.

use kicad_core::stroke_params::{LineStyle, StrokeParams, StrokeParamsParser};
use kicad_core::{Color4D, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_geom::{EdaAngle, Vector2I, is_straight_arc};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;
use kicad_sexpr::keywords::strokeparams_t;

use super::PcbParser;
use crate::ctx::MutEnv;
use crate::eda_shape::{FillT, ShapeT};
use crate::item::ParentRef;
use crate::item_ref::AnyItem;
use crate::line_ending::{LineEnding, LineEndingStyle};
use crate::pcb_shape::{DEFAULT_LINE_WIDTH, PcbShape};

impl PcbParser {
    /// Port of `parseLineEnding( LINE_ENDING& aEnding )`: the current token is `start_shape`
    /// or `end_shape`; reads the style and the optional `(length ..)`, `(width ..)` and
    /// `(stroke ..)` up to and including the closing `)`.
    pub(crate) fn parse_line_ending(&mut self, ending: &mut LineEnding) -> Result<(), IoError> {
        // Current token is T_start_shape or T_end_shape. Next token is the style.
        let mut token = self.lexer.next_tok()?;

        match token {
            T::T_arrow => ending.set_style(LineEndingStyle::Arrow),
            T::T_circle => ending.set_style(LineEndingStyle::Circle),
            T::T_square => ending.set_style(LineEndingStyle::Square),
            T::T_arrow_open => ending.set_style(LineEndingStyle::ArrowOpen),
            T::T_none => ending.set_style(LineEndingStyle::None),
            _ => {
                return Err(self
                    .lexer
                    .expecting_str("arrow, circle, square, arrow_open, or none"));
            }
        }

        // Parse optional sub-tokens
        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_length => {
                    ending.set_length(self.parse_board_units_str("length")?);
                    self.lexer.need_right()?;
                }
                T::T_width => {
                    ending.set_width(self.parse_board_units_str("width")?);
                    self.lexer.need_right()?;
                }
                T::T_stroke => {
                    let mut stroke = StrokeParams::default();
                    self.parse_stroke(&mut stroke)?;
                    ending.set_stroke(stroke);
                }
                _ => return Err(self.lexer.expecting_str("length, width, or stroke")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// `STROKE_PARAMS_PARSER strokeParser( reader, pcbIUScale.IU_PER_MM );
    /// strokeParser.SyncLineReaderWith( *this ); strokeParser.ParseStroke( aStroke );
    /// SyncLineReaderWith( strokeParser );`: call with `CurTok() == T_stroke`; reads up to and
    /// including the closing `)` with the `stroke_params.keywords` table.
    pub(crate) fn parse_stroke(&mut self, stroke: &mut StrokeParams) -> Result<(), IoError> {
        // STROKE_PARAMS_PARSER( LINE_READER*, int iuPerMM ): IU_PER_MM (1e6) converted to int.
        let parser = StrokeParamsParser::new(PCB_IU_SCALE.iu_per_mm as i32);

        self.lexer
            .with_sub_lexer::<strokeparams_t::T, _, _>(|sub| parser.parse_stroke(sub, stroke))
    }

    /// `GetParentFootprint() != nullptr` for an item whose parent is `parent` (the parent
    /// chain reaches a footprint).
    fn parent_has_footprint(&self, parent: ParentRef) -> bool {
        let mut parent = parent;

        // A parent chain is at most footprint -> table -> cell deep; bound the walk anyway.
        for _ in 0..16 {
            let ParentRef::Item(id) = parent else {
                return false;
            };

            match self.board.get(id) {
                Some(AnyItem::Footprint(_)) => return true,
                Some(item) => parent = item.as_dyn().base().parent,
                None => return false,
            }
        }

        false
    }

    /// `aParent && aParent->Type() == PCB_FOOTPRINT_T`.
    fn parent_is_footprint(&self, parent: ParentRef) -> bool {
        match parent {
            ParentRef::Item(id) => matches!(self.board.get(id), Some(AnyItem::Footprint(_))),
            _ => false,
        }
    }

    /// The shape-kind keyword, then `[locked] (start x y)`-style first list (`T_start` /
    /// `T_center` / ...): reads the legacy bare `locked` and the `(` and returns the next
    /// token.
    fn shape_head(&mut self, shape: &mut PcbShape) -> Result<T, IoError> {
        let mut token = self.lexer.next_tok()?;

        if token == T::T_locked {
            shape.base.item.is_locked = true;
            token = self.lexer.next_tok()?;
        }

        if token != T::T_LEFT {
            return Err(self.lexer.expecting(T::T_LEFT));
        }

        self.lexer.next_tok()
    }

    /// `pt.x = parseBoardUnits( "X coordinate" ); pt.y = parseBoardUnits( "Y coordinate" );`
    fn parse_xy_coordinates(&mut self) -> Result<Vector2I, IoError> {
        let x = self.parse_board_units_str("X coordinate")?;
        let y = self.parse_board_units_str("Y coordinate")?;
        Ok(Vector2I::new(x, y))
    }

    /// `NeedRIGHT(); NeedLEFT(); token = NextTok(); if( token != aExpected ) Expecting(
    /// aExpected );`
    fn next_sub_list(&mut self, expected: T) -> Result<(), IoError> {
        self.lexer.need_right()?;
        self.lexer.need_left()?;
        let token = self.lexer.next_tok()?;

        if token != expected {
            return Err(self.lexer.expecting(expected));
        }

        Ok(())
    }

    /// Port of `parsePCB_SHAPE( BOARD_ITEM* aParent )`: `gr_*` / `fp_*` shapes (and pad
    /// primitives, `parent == ParentRef::None`).
    pub(crate) fn parse_pcb_shape(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<PcbShape>, IoError> {
        let cur = self.lexer.cur_tok();

        debug_assert!(
            matches!(
                cur,
                T::T_fp_arc
                    | T::T_fp_circle
                    | T::T_fp_curve
                    | T::T_fp_rect
                    | T::T_fp_line
                    | T::T_fp_poly
                    | T::T_fp_ellipse
                    | T::T_fp_ellipse_arc
                    | T::T_gr_arc
                    | T::T_gr_circle
                    | T::T_gr_curve
                    | T::T_gr_rect
                    | T::T_gr_bbox
                    | T::T_gr_line
                    | T::T_gr_poly
                    | T::T_gr_vector
                    | T::T_gr_ellipse
                    | T::T_gr_ellipse_arc
            ),
            "Cannot parse {} as PCB_SHAPE.",
            self.lexer.token_string(cur)
        );

        let mut stroke = StrokeParams::new(0, LineStyle::Solid, Color4D::UNSPECIFIED);
        let mut shape = PcbShape::new(&mut self.board.kiid_generator, ShapeT::Segment);
        shape.base.item.parent = parent;

        let env: MutEnv = self.board.child_env(parent);

        let mut ellipse_center = Vector2I::new(0, 0);
        let mut ellipse_major = 0;
        let mut ellipse_minor = 0;
        let mut ellipse_rotation = EdaAngle::from_degrees(0.0);
        let mut ellipse_start = EdaAngle::from_degrees(0.0);
        let mut ellipse_end = EdaAngle::from_degrees(0.0);

        match cur {
            T::T_gr_arc | T::T_fp_arc => {
                shape.set_shape(&env, ShapeT::Arc);
                let token = self.shape_head(&mut shape)?;

                if self.required_version <= super::super::LEGACY_ARC_FORMATTING {
                    // In legacy files the start keyword actually gives the arc center...
                    if token != T::T_start {
                        return Err(self.lexer.expecting(T::T_start));
                    }

                    let pt = self.parse_xy_coordinates()?;
                    shape.set_center(&env, pt);

                    // ... and the end keyword gives the start point of the arc
                    self.next_sub_list(T::T_end)?;

                    let pt = self.parse_xy_coordinates()?;
                    shape.set_start(&env, pt);

                    self.next_sub_list(T::T_angle)?;

                    let angle = EdaAngle::from_degrees(self.parse_double_expected("arc angle")?);

                    // PCB_SHAPE::SetArcAngleAndEnd: the EDA_SHAPE body, then syncLibCoords().
                    shape.shape.set_arc_angle_and_end(angle, true);
                    shape.sync_lib_coords(&env);
                    self.lexer.need_right()?;
                } else {
                    if token != T::T_start {
                        return Err(self.lexer.expecting(T::T_start));
                    }

                    let arc_start = self.parse_xy_coordinates()?;

                    self.next_sub_list(T::T_mid)?;

                    let arc_mid = self.parse_xy_coordinates()?;

                    self.next_sub_list(T::T_end)?;

                    let arc_end = self.parse_xy_coordinates()?;
                    self.lexer.need_right()?;

                    set_arc_or_segment(&mut shape, &env, arc_start, arc_mid, arc_end);
                }
            }

            T::T_gr_circle | T::T_fp_circle => {
                shape.set_shape(&env, ShapeT::Circle);
                let token = self.shape_head(&mut shape)?;

                if token != T::T_center {
                    return Err(self.lexer.expecting(T::T_center));
                }

                let pt = self.parse_xy_coordinates()?;
                shape.set_start(&env, pt);

                self.next_sub_list(T::T_end)?;

                let pt = self.parse_xy_coordinates()?;
                shape.set_end(&env, pt);
                self.lexer.need_right()?;
            }

            T::T_gr_curve | T::T_fp_curve => {
                shape.set_shape(&env, ShapeT::Bezier);
                let token = self.shape_head(&mut shape)?;

                if token != T::T_pts {
                    return Err(self.lexer.expecting(T::T_pts));
                }

                let pt = self.parse_xy()?;
                shape.set_start(&env, pt);
                let pt = self.parse_xy()?;
                shape.set_bezier_c1(&env, pt);
                let pt = self.parse_xy()?;
                shape.set_bezier_c2(&env, pt);
                let pt = self.parse_xy()?;
                shape.set_end(&env, pt);

                if self.board_present {
                    let max_error = self.board.design_settings().max_error;
                    shape.shape.rebuild_bezier_to_segments_points_list(max_error);
                } else {
                    shape.shape.rebuild_bezier_to_segments_points_list(ARC_HIGH_DEF);
                }

                self.lexer.need_right()?;
            }

            T::T_gr_bbox | T::T_gr_rect | T::T_fp_rect => {
                shape.set_shape(&env, ShapeT::Rectangle);
                let token = self.shape_head(&mut shape)?;

                if token != T::T_start {
                    return Err(self.lexer.expecting(T::T_start));
                }

                let pt = self.parse_xy_coordinates()?;
                shape.set_start(&env, pt);

                self.next_sub_list(T::T_end)?;

                let pt = self.parse_xy_coordinates()?;
                shape.set_end(&env, pt);

                if self.parent_is_footprint(parent) {
                    // Footprint shapes are stored in board-relative coordinates, but we want
                    // the normalization to remain in footprint-relative coordinates.
                } else {
                    shape.normalize();
                }

                self.lexer.need_right()?;
            }

            T::T_gr_vector | T::T_gr_line | T::T_fp_line => {
                shape.set_shape(&env, ShapeT::Segment);
                let token = self.shape_head(&mut shape)?;

                if token != T::T_start {
                    return Err(self.lexer.expecting(T::T_start));
                }

                let pt = self.parse_xy_coordinates()?;
                shape.set_start(&env, pt);

                self.next_sub_list(T::T_end)?;

                let pt = self.parse_xy_coordinates()?;
                shape.set_end(&env, pt);
                self.lexer.need_right()?;
            }

            T::T_gr_poly | T::T_fp_poly => {
                shape.set_shape(&env, ShapeT::Poly);
                shape.shape.set_poly_points(&[]);

                let token = self.shape_head(&mut shape)?;

                if token != T::T_pts {
                    return Err(self.lexer.expecting(T::T_pts));
                }

                // SHAPE_LINE_CHAIN& outline = shape->GetPolyShape().Outline( 0 );
                while self.lexer.next_tok()? != T::T_RIGHT {
                    self.parse_outline_points(shape.shape.poly.outline(0))?;
                }
            }

            T::T_gr_ellipse | T::T_fp_ellipse => shape.set_shape(&env, ShapeT::Ellipse),

            T::T_gr_ellipse_arc | T::T_fp_ellipse_arc => {
                shape.set_shape(&env, ShapeT::EllipseArc);
            }

            _ => {
                if self.parent_is_footprint(parent) {
                    return Err(self.lexer.expecting_str(
                        "fp_arc, fp_circle, fp_curve, fp_ellipse, fp_ellipse_arc, fp_line, \
                         fp_poly or fp_rect",
                    ));
                }

                return Err(self.lexer.expecting_str(
                    "gr_arc, gr_circle, gr_curve, gr_ellipse, gr_ellipse_arc, gr_vector, \
                     gr_line, gr_poly, gr_rect or gr_bbox",
                ));
            }
        }

        let mut found_fill = false;
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                // legacy token; ignore value
                T::T_angle => {
                    self.parse_double_expected("arc angle")?;
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    shape.set_layer(&env, layer);
                    self.lexer.need_right()?;
                }
                T::T_layers => {
                    let layers = self.parse_board_item_layers_as_mask()?;
                    shape.set_layer_set(&env, layers);
                }
                T::T_solder_mask_margin => {
                    let margin = self.parse_board_units_str("local solder mask margin value")?;
                    shape.set_local_solder_mask_margin(Some(margin));
                    self.lexer.need_right()?;
                }
                // legacy token
                T::T_width => {
                    let width = self.parse_board_units_tok(
                        T::T_width,
                        kicad_core::EdaDataType::Distance,
                    )?;
                    stroke.set_width(width);
                    self.lexer.need_right()?;
                }
                T::T_radius => {
                    let radius = self.parse_board_units_str("corner radius")?;
                    shape.set_corner_radius(&env, radius);
                    self.lexer.need_right()?;
                }
                T::T_stroke => self.parse_stroke(&mut stroke)?,
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    shape.base.item.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_fill => {
                    found_fill = true;

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token == T::T_LEFT {
                            token = self.lexer.next_tok()?;
                        }

                        match token {
                            // T_yes was used to indicate filling when first introduced, so treat
                            // it like a solid fill since that was the only fill available at the
                            // time.
                            T::T_yes | T::T_solid => shape.shape.set_fill_mode(FillT::FilledShape),
                            T::T_none | T::T_no => shape.shape.set_fill_mode(FillT::NoFill),
                            T::T_hatch => shape.shape.set_fill_mode(FillT::Hatch),
                            T::T_reverse_hatch => shape.shape.set_fill_mode(FillT::ReverseHatch),
                            T::T_cross_hatch => shape.shape.set_fill_mode(FillT::CrossHatch),
                            _ => {
                                return Err(self.lexer.expecting_str(
                                    "yes, no, solid, none, hatch, reverse_hatch or cross_hatch",
                                ));
                            }
                        }

                        token = self.lexer.next_tok()?;
                    }
                }
                // legacy token; ignore value
                T::T_status => {
                    self.parse_hex()?;
                    self.lexer.need_right()?;
                }
                // Handle "(locked)" from 5.99 development, and "(locked yes)" from modern times
                T::T_locked => {
                    shape.base.item.is_locked = self.parse_maybe_absent_bool(true)?;
                }
                T::T_net => self.parse_net(&mut shape)?,
                T::T_center => {
                    ellipse_center = self.parse_xy_coordinates()?;
                    self.lexer.need_right()?;
                }
                T::T_major_radius => {
                    ellipse_major = self.parse_board_units_str("major radius")?;
                    self.lexer.need_right()?;
                }
                T::T_minor_radius => {
                    ellipse_minor = self.parse_board_units_str("minor radius")?;
                    self.lexer.need_right()?;
                }
                T::T_rotation_angle => {
                    ellipse_rotation =
                        EdaAngle::from_degrees(self.parse_double_expected("rotation angle")?);
                    self.lexer.need_right()?;
                }
                T::T_start_angle => {
                    ellipse_start = EdaAngle::from_degrees(self.parse_double_expected("start angle")?);
                    self.lexer.need_right()?;
                }
                T::T_end_angle => {
                    ellipse_end = EdaAngle::from_degrees(self.parse_double_expected("end angle")?);
                    self.lexer.need_right()?;
                }
                T::T_start_shape => {
                    let mut ending = LineEnding::default();
                    self.parse_line_ending(&mut ending)?;
                    shape.shape.set_start_ending(ending);
                }
                T::T_end_shape => {
                    let mut ending = LineEnding::default();
                    self.parse_line_ending(&mut ending)?;
                    shape.shape.set_end_ending(ending);
                }
                T::T_custom_property => self.parse_custom_property_item(&mut shape)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "layer, width, fill, tstamp, uuid, locked, net, status, \
                         solder_mask_margin, center, major_radius, minor_radius, rotation_angle, \
                         start_angle, end_angle, start_shape, end_shape, or custom_property",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if !found_fill {
            // Legacy versions didn't have a filled flag but allowed some shapes to indicate they
            // should be filled by specifying a 0 stroke-width.
            if stroke.width() == 0
                && (shape.shape.shape() == ShapeT::Rectangle
                    || shape.shape.shape() == ShapeT::Circle)
            {
                shape.shape.set_filled(true);
            } else if shape.shape.shape() == ShapeT::Poly
                && shape.base.item.layer != PcbLayerId::EdgeCuts
            {
                // Polygons on non-Edge_Cuts layers were always filled.
                shape.shape.set_filled(true);
            }
        }

        // Only filled shapes may have a zero line-width.  This is not permitted in KiCad but some
        // external tools can generate invalid files.
        if stroke.width() <= 0 && !shape.shape.is_any_fill() {
            stroke.set_width(PCB_IU_SCALE.mm_to_iu(DEFAULT_LINE_WIDTH));
        }

        let stroke_width = stroke.width();
        shape.set_stroke(&env, stroke);

        let has_parent_fp = self.parent_has_footprint(parent);

        if has_parent_fp {
            shape.set_lib_stroke_width(stroke_width);
        }

        let kind = shape.shape.shape();

        if kind == ShapeT::Ellipse || kind == ShapeT::EllipseArc {
            shape.set_library_ellipse(
                &env,
                ellipse_center,
                ellipse_major,
                ellipse_minor,
                ellipse_rotation,
                ellipse_start,
                ellipse_end,
            );

            if has_parent_fp {
                shape.rebake_from_lib(&env);
            }
        } else if has_parent_fp {
            let lib_start = shape.shape.start();
            let lib_end = shape.shape.end();
            let lib_arc_mid = if kind == ShapeT::Arc {
                shape.shape.arc_mid()
            } else {
                Vector2I::new(0, 0)
            };

            shape.override_lib_coords(lib_start, lib_end, lib_arc_mid);

            if kind == ShapeT::Rectangle {
                let radius = shape.shape.corner_radius();
                shape.override_lib_corner_radius(radius);
            }

            if kind == ShapeT::Bezier {
                let (c1, c2) = (shape.shape.bezier_c1(), shape.shape.bezier_c2());
                shape.override_lib_bezier(c1, c2);
            }

            if kind == ShapeT::Poly {
                let poly = shape.shape.poly_shape().clone();
                shape.override_lib_poly(poly);
            }

            shape.rebake_from_lib(&env);
        }

        Ok(Some(shape))
    }
}

/// `shape->SetArcOrSegment( aStart, aMid, aEnd )` on a `PCB_SHAPE`: the `EDA_SHAPE` body, whose
/// calls of the virtual `SetShape` / `SetStart` / `SetEnd` / `SetArcGeometry` reach the
/// `PCB_SHAPE` overrides (which keep the library-frame copy in sync).
fn set_arc_or_segment(
    shape: &mut PcbShape,
    env: &MutEnv,
    start: Vector2I,
    mid: Vector2I,
    end: Vector2I,
) {
    if is_straight_arc(start, mid, end) {
        shape.set_shape(env, ShapeT::Segment);
        shape.set_start(env, start);
        shape.set_end(env, end);
    } else {
        shape.set_arc_geometry(env, start, mid, end);
    }
}
