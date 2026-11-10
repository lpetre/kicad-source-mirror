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

//! Port of the reader half of `pcbnew/pcb_plot_params.cpp` (package PCBIO-04):
//! `PCB_PLOT_PARAMS::Parse` and `PCB_PLOT_PARAMS_PARSER` (`Parse`, `parseBool`, `parseInt`,
//! `parseDouble`, `skipCurrent`) on the `PCBPLOTPARAMS_T` keyword table. The legacy layer
//! renumbering (`remapLegacyLayerLSET`) is `kicad_core::legacy_layers`.

use kicad_core::base_set::BaseSet;
use kicad_core::legacy_layers::{LEGACY_PCB_LAYER_ID_COUNT, remap_legacy_layer_lset};
use kicad_core::plot_enums::{DrillMarks, DxfUnits, PlotFormat, PlotTextMode};
use kicad_core::{Lset, PcbLayerId};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcbplotparams_t::{PcbPlotParamsLexer, T};

use crate::pcb_plot_params::{
    GBR_DEFAULT_PRECISION, PcbPlotParams, SVG_PRECISION_MAX, SVG_PRECISION_MIN,
};

/// `MIN_PNG_DPI` (`include/plotters/plotter_png.h`).
const MIN_PNG_DPI: i32 = 72;
/// `MAX_PNG_DPI` (`include/plotters/plotter_png.h`).
const MAX_PNG_DPI: i32 = 2400;

/// Port of `PCB_PLOT_PARAMS_PARSER`: the `PCB_PLOT_PARAMS_LEXER` plus the board file version
/// it was constructed with (`PCB_PLOT_PARAMS_PARSER( LINE_READER*, int aBoardFileVersion )`).
pub(crate) struct PcbPlotParamsParser<'a> {
    pub lexer: &'a mut PcbPlotParamsLexer,
    /// `m_boardFileVersion`.
    pub board_file_version: i32,
}

impl PcbPlotParamsParser<'_> {
    /// Port of `PCB_PLOT_PARAMS_PARSER::Parse( PCB_PLOT_PARAMS* )` (also reached through
    /// `PCB_PLOT_PARAMS::Parse`): reads up to and including the `)` closing the
    /// `(pcbplotparams ...)` list.
    pub(crate) fn parse(&mut self, params: &mut PcbPlotParams) -> Result<(), IoError> {
        loop {
            let mut token = self.lexer.next_tok()?;

            if token == T::T_RIGHT {
                break;
            }

            if token == T::T_EOF {
                return Err(self.lexer.unexpected(T::T_EOF));
            }

            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            if token == T::T_pcbplotparams {
                continue;
            }

            let mut skip_right = false;

            match token {
                T::T_layerselection => {
                    let token = self.lexer.need_symbol_or_number()?;
                    let cur = self.lexer.cur_str().to_string();

                    if token == T::T_NUMBER {
                        // pretty 3 format had legacy Cu stack.
                        //  It's not possible to convert a legacy Cu layer number to a new Cu
                        //  layer number without knowing the number or total Cu layers in the
                        //  legacy board.  We do not have that information here, so simply set
                        //  all layers ON.  User can turn them off in the UI.
                        let mut selection =
                            Lset::from_layers(&[PcbLayerId::FSilkS, PcbLayerId::BSilkS]);
                        selection |= Lset::all_cu_mask();
                        params.layer_selection = selection;
                    } else if find_first_of_0x_is_0(&cur) {
                        // pretty ver. 4.
                        // The layers were renumbered in 5e0abadb23425765e164f49ee2f893e94ddb97fc,
                        // but there wasn't a board file version change with it, so this value is
                        // the one immediately after that happened.
                        if self.board_file_version < 20240819 {
                            let mut legacy_lset = BaseSet::new(LEGACY_PCB_LAYER_ID_COUNT);

                            // skip the leading 2 0x bytes.
                            legacy_lset.parse_hex_bytes(after_0x(&cur));
                            params.set_layer_selection(&remap_legacy_layer_lset(&legacy_lset));
                        } else {
                            // skip the leading 2 0x bytes.
                            params.layer_selection.parse_hex_bytes(after_0x(&cur));
                        }
                    } else {
                        return Err(self.lexer.expecting_str("integer or hex layerSelection"));
                    }
                }

                T::T_plot_on_all_layers_selection => {
                    self.lexer.need_symbol_or_number()?;
                    let cur = self.lexer.cur_str().to_string();

                    if find_first_of_0x_is_0(&cur) {
                        // The layers were renumbered in 5e0abadb23425765e164f49ee2f893e94ddb97fc,
                        // but there wasn't a board file version change with it, so this value
                        // is the one immediately after that happened.
                        let layers = if self.board_file_version < 20240819 {
                            let mut legacy_lset = BaseSet::new(LEGACY_PCB_LAYER_ID_COUNT);

                            // skip the leading 2 0x bytes.
                            legacy_lset.parse_hex_bytes(after_0x(&cur));

                            remap_legacy_layer_lset(&legacy_lset)
                        } else {
                            let mut parsed = Lset::new();

                            // skip the leading 2 0x bytes.
                            parsed.parse_hex_bytes(after_0x(&cur));
                            parsed
                        };

                        params.set_plot_on_all_layers_sequence(layers.seq_stackup_for_plotting().0);
                    } else {
                        return Err(self.lexer.expecting_str("hex plot_on_all_layers_selection"));
                    }
                }

                T::T_disableapertmacros => {
                    params.gerber_disable_apert_macros = self.parse_bool()?
                }
                T::T_usegerberextensions => {
                    params.use_gerber_protel_extensions = self.parse_bool()?
                }
                T::T_usegerberattributes => params.use_gerber_x2_format = self.parse_bool()?,
                T::T_usegerberadvancedattributes => {
                    params.include_gerber_netlist_info = self.parse_bool()?
                }
                T::T_creategerberjobfile => params.create_gerber_job_file = self.parse_bool()?,
                T::T_gerberprecision => {
                    params.gerber_precision =
                        self.parse_int(GBR_DEFAULT_PRECISION - 1, GBR_DEFAULT_PRECISION)?;
                }
                T::T_dashed_line_dash_ratio => {
                    params.dashed_line_dash_ratio = self.parse_double()?
                }
                T::T_dashed_line_gap_ratio => params.dashed_line_gap_ratio = self.parse_double()?,
                T::T_svgprecision => {
                    // parseInt returns an int stored in an unsigned member
                    params.svg_precision =
                        self.parse_int(SVG_PRECISION_MIN as i32, SVG_PRECISION_MAX as i32)? as u32;
                }
                T::T_pngdpi => params.png_dpi = self.parse_int(MIN_PNG_DPI, MAX_PNG_DPI)?,
                T::T_pngantialias => params.png_antialias = self.parse_bool()?,
                T::T_svguseinch => {
                    self.parse_bool()?; // Unused. For compatibility
                }
                T::T_psa4output => params.a4_output = self.parse_bool()?,
                T::T_excludeedgelayer => {
                    if !self.parse_bool()? {
                        params
                            .plot_on_all_layers_sequence
                            .push(PcbLayerId::EdgeCuts);
                    }
                }
                T::T_plotframeref => params.plot_drawing_sheet = self.parse_bool()?,
                T::T_viasonmask => params.plot_via_on_mask_layer = Some(self.parse_bool()?),
                T::T_useauxorigin => params.use_aux_origin = self.parse_bool()?,
                T::T_mode | T::T_hpglpennumber | T::T_hpglpenspeed | T::T_hpglpenoverlay => {
                    // HPGL is no longer supported
                    self.parse_int(i32::MIN, i32::MAX)?;
                }
                T::T_pdf_front_fp_property_popups => {
                    params.pdf_front_fp_property_popups = self.parse_bool()?
                }
                T::T_pdf_back_fp_property_popups => {
                    params.pdf_back_fp_property_popups = self.parse_bool()?
                }
                T::T_pdf_metadata => params.pdf_metadata = self.parse_bool()?,
                T::T_pdf_single_document => params.pdf_single = self.parse_bool()?,
                T::T_dxfpolygonmode => params.dxf_polygon_mode = self.parse_bool()?,
                T::T_dxfimperialunits => {
                    params.dxf_units = if self.parse_bool()? {
                        DxfUnits::Inch
                    } else {
                        DxfUnits::Mm
                    };
                }
                T::T_dxfusepcbnewfont => {
                    params.text_mode = if self.parse_bool()? {
                        PlotTextMode::Default
                    } else {
                        PlotTextMode::Native
                    };
                }
                T::T_pscolor => {
                    self.lexer.need_symbol()?; // This actually was never used...
                }
                T::T_psnegative => params.negative = self.parse_bool()?,
                T::T_plot_black_and_white => params.black_and_white = self.parse_bool()?,
                T::T_plotinvisibletext => {
                    // legacy token; no longer supported
                    self.parse_bool()?;
                }
                T::T_sketchpadsonfab => params.sketch_pads_on_fab_layers = self.parse_bool()?,
                T::T_plotpadnumbers => params.plot_pad_numbers = self.parse_bool()?,
                T::T_hidednponfab => params.hide_dnp_fps_on_fab_layers = self.parse_bool()?,
                T::T_sketchdnponfab => params.sketch_dnp_fps_on_fab_layers = self.parse_bool()?,
                T::T_crossoutdnponfab => {
                    params.crossout_dnp_fps_on_fab_layers = self.parse_bool()?
                }
                T::T_subtractmaskfromsilk => params.subtract_mask_from_silk = self.parse_bool()?,
                T::T_outputformat => {
                    let value = self.parse_int(
                        PlotFormat::FIRST_FORMAT as i32,
                        PlotFormat::LAST_FORMAT as i32,
                    )?;
                    // In range by the clamping of parseInt.
                    params.format = PlotFormat::from_i32(value).unwrap_or_default();
                }
                T::T_mirror => params.mirror = self.parse_bool()?,
                T::T_drillshape => {
                    let value = self.parse_int(0, 2)?;
                    params.drill_marks = DrillMarks::from_i32(value).unwrap_or_default();
                }
                T::T_scaleselection => params.scale_selection = self.parse_int(0, 4)?,
                T::T_outputdirectory => {
                    self.lexer.need_symbol_or_number()?; // a dir name can be like a number
                    params.output_directory = self.lexer.from_utf8();
                }
                _ => {
                    self.skip_current()?; // skip unknown or outdated plot parameter
                    skip_right = true; // the closing right token is already read.
                }
            }

            if !skip_right {
                self.lexer.need_right()?;
            }
        }

        Ok(())
    }

    /// Port of `PCB_PLOT_PARAMS_PARSER::parseBool`: `true`, `false`, `yes` or `no`.
    fn parse_bool(&mut self) -> Result<bool, IoError> {
        let token = self.lexer.need_symbol()?;

        match token {
            T::T_false | T::T_no => Ok(false),
            T::T_true | T::T_yes => Ok(true),
            _ => Err(self.lexer.expecting_str("true, false, yes, or no")),
        }
    }

    /// Port of `PCB_PLOT_PARAMS_PARSER::parseInt( aMin, aMax )`: the next token must be a
    /// number; `atoi` of it, clamped to `[aMin, aMax]`.
    fn parse_int(&mut self, min: i32, max: i32) -> Result<i32, IoError> {
        let token = self.lexer.next_tok()?;

        if token != T::T_NUMBER {
            return Err(self.lexer.expecting(T::T_NUMBER));
        }

        let mut val = self.lexer.parse_int();

        if val < min {
            val = min;
        } else if val > max {
            val = max;
        }

        Ok(val)
    }

    /// Port of `PCB_PLOT_PARAMS_PARSER::parseDouble`: the next token must be a number.
    fn parse_double(&mut self) -> Result<f64, IoError> {
        let token = self.lexer.next_tok()?;

        if token != T::T_NUMBER {
            return Err(self.lexer.expecting(T::T_NUMBER));
        }

        self.lexer.parse_double()
    }

    /// Port of `PCB_PLOT_PARAMS_PARSER::skipCurrent`: skip the current token level, i.e.
    /// search for the RIGHT parenthesis which closes the current description.
    fn skip_current(&mut self) -> Result<(), IoError> {
        let mut curr_level = 0;

        loop {
            let token = self.lexer.next_tok()?;

            if token == T::T_EOF {
                return Ok(());
            }

            if token == T::T_LEFT {
                curr_level -= 1;
            }

            if token == T::T_RIGHT {
                curr_level += 1;

                if curr_level > 0 {
                    return Ok(());
                }
            }
        }
    }
}

/// `cur.find_first_of( "0x" ) == 0`: the first character is `0` or `x`.
fn find_first_of_0x_is_0(cur: &str) -> bool {
    matches!(cur.as_bytes().first(), Some(b'0' | b'x'))
}

/// `cur.c_str() + 2, cur.size() - 2`: the text after the first two bytes. For a one-byte
/// token the C++ passes a negative count, for which `BASE_SET::ParseHex` reads nothing.
fn after_0x(cur: &str) -> &[u8] {
    cur.as_bytes().get(2..).unwrap_or(&[])
}
